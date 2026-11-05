//! Tauri 2 应用壳：命令注册、事件派发、转录运行时。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use moonshine_core::audio::{host_name, list_input_devices, probe_devices, AudioCapture};
use moonshine_core::diarization::{Diarizer, DualChannelDiarizer};
use moonshine_core::engine::{
    self, EngineConfig, EngineKind, RemoteCredentials, Segment, Source,
};
use moonshine_core::export::{self, ExportFormat};
use moonshine_core::session::Session;
use moonshine_core::summary::{self, SummaryConfig};
use moonshine_core::vad::{rms, EnergyVad, Vad};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::{DialogExt, FilePath};

/// 单路音频块大小：160ms @ 16kHz = 2560 样本（端到端 <300ms 目标）。
const CHUNK_SAMPLES: usize = 2560;

#[derive(Default)]
struct AppState {
    running: Arc<AtomicBool>,
    stop_flag: Arc<AtomicBool>,
    session: Arc<Mutex<Session>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartConfig {
    mic_device_index: i32,
    loopback_device_index: i32,
    /// 设备名优先定位，避免枚举与打开之间设备顺序变化导致 index 错位。
    mic_device_name: Option<String>,
    loopback_device_name: Option<String>,
    engine: String,
    model: String,
    language: String,
    vad_enabled: bool,
    dual_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
struct TranscriptEvent {
    source: String,
    speaker: Option<String>,
    text: String,
    start: f32,
    end: f32,
    is_final: bool,
    ts: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveRequest {
    filename: String,
    content: String,
}

fn parse_engine_kind(s: &str) -> Result<EngineKind, String> {
    match s {
        "faster_whisper" | "whisper" => Ok(EngineKind::FasterWhisper),
        "moonshine" => Ok(EngineKind::Moonshine),
        "funasr" | "paraformer" => Ok(EngineKind::Funasr),
        "volcengine" | "volc" => Ok(EngineKind::Volcengine),
        "tencent" => Ok(EngineKind::Tencent),
        "alicloud" | "aliyun" => Ok(EngineKind::Alicloud),
        other => Err(format!("未知转写引擎: {other}")),
    }
}

/// 定位本地引擎脚本。
///
/// 优先级：环境变量 > 开发目录 > 安装后的资源目录（由 `bundle.resources` 复制）。
fn default_engine_script() -> PathBuf {
    if let Ok(p) = std::env::var("MOONSHINE_ENGINE_SCRIPT") {
        return PathBuf::from(p);
    }

    // 开发模式：工作目录为项目根
    let dev = PathBuf::from("python/engine_service.py");
    if dev.is_file() {
        return dev.clone();
    }

    // 安装后：可执行文件同目录 / Tauri 资源目录（不同平台路径不同）
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidates = [
                dir.join("resources/python/engine_service.py"),
                dir.join("resources/_up_/python/engine_service.py"),
                dir.join("python/engine_service.py"),
                dir.join("../Resources/python/engine_service.py"),
                dir.join("../Resources/_up_/python/engine_service.py"),
            ];
            for candidate in candidates {
                if candidate.is_file() {
                    return candidate;
                }
            }
        }
    }

    dev
}

/// ASR 凭证存储路径（app 配置目录）。
fn asr_settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("无法定位配置目录: {e}"))?;
    Ok(dir.join("asr-settings.json"))
}

/// 从磁盘加载云端 ASR 凭证（不存在时返回空配置）。
fn load_remote_credentials(app: &AppHandle) -> Result<RemoteCredentials, String> {
    let path = asr_settings_path(app)?;
    if !path.exists() {
        return Ok(RemoteCredentials::default());
    }
    let json = std::fs::read_to_string(&path).map_err(|e| format!("读取凭证失败: {e}"))?;
    serde_json::from_str(&json).map_err(|e| format!("解析凭证失败: {e}"))
}

// ========================
// 命令
// ========================

#[tauri::command]
fn list_devices() -> Result<Vec<moonshine_core::audio::AudioDevice>, String> {
    list_input_devices()
}

#[tauri::command]
fn get_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// 音频诊断：返回宿主、设备清单，以及逐个设备真实打开输入流的结果与原始错误。
#[tauri::command]
fn probe_audio_devices() -> Result<serde_json::Value, String> {
    let devices = list_input_devices()?;
    let probes = probe_devices()?;
    Ok(serde_json::json!({
        "host": host_name(),
        "targetSampleRate": moonshine_core::audio::TARGET_SAMPLE_RATE,
        "devices": devices,
        "probes": probes,
    }))
}

#[tauri::command]
fn start_transcription(
    app: AppHandle,
    state: State<'_, AppState>,
    config: StartConfig,
) -> Result<(), String> {
    if state.running.load(Ordering::SeqCst) {
        return Err("已在录制中".to_string());
    }

    let kind = parse_engine_kind(&config.engine)?;
    // 云端引擎需从本地配置加载 BYOK 凭证
    let remote = if kind.is_remote() {
        Some(load_remote_credentials(&app)?)
    } else {
        None
    };
    let engine_config = EngineConfig {
        kind,
        model: config.model.clone(),
        language: if config.language.is_empty() {
            None
        } else {
            Some(config.language.clone())
        },
        python: if cfg!(windows) { "python".into() } else { "python3".into() },
        script: default_engine_script(),
        remote,
    };

    // 重置会话
    {
        let mut session = state.session.lock().unwrap();
        session.clear();
    }

    state.running.store(true, Ordering::SeqCst);
    state.stop_flag.store(false, Ordering::SeqCst);

    let app_handle = app.clone();
    let running = state.running.clone();
    let stop_flag = state.stop_flag.clone();
    let session = state.session.clone();

    std::thread::spawn(move || {
        if let Err(e) = run_transcription(
            &app_handle,
            &config,
            &engine_config,
            &running,
            &stop_flag,
            &session,
        ) {
            let _ = app_handle.emit(
                "log",
                format!("[ERROR] {e}"),
            );
            let _ = app_handle.emit("status", "stopped");
            running.store(false, Ordering::SeqCst);
        }
    });

    Ok(())
}

#[tauri::command]
fn stop_transcription(state: State<'_, AppState>) -> Result<(), String> {
    state.stop_flag.store(true, Ordering::SeqCst);
    Ok(())
}

// ========================
// 云端 ASR 设置（BYOK）
// ========================

/// 云端 ASR 服务商元数据（服务商列表、所需 key 字段、可选模型）。
#[tauri::command]
fn get_asr_providers() -> serde_json::Value {
    serde_json::json!([
        {
            "id": "volcengine",
            "name": "火山引擎",
            "needsKey": true,
            "keyFields": [
                { "key": "accessKeyId", "label": "AccessKey ID", "placeholder": "AKLT...", "type": "text" },
                { "key": "secretKey", "label": "Secret Key", "placeholder": "火山引擎 SecretKey", "type": "password" },
                { "key": "appId", "label": "App ID", "placeholder": "数字应用 ID", "type": "text" }
            ],
            "models": [
                { "id": "bigmodel", "name": "豆包流式语音识别 2.0（中文）" },
                { "id": "bigmodel_en", "name": "豆包流式（英语）" }
            ],
            "console": "https://console.volcengine.com/speech/app",
            "consoleLabel": "火山引擎 · 语音技术控制台",
            "docUrl": "https://www.volcengine.com/docs/6561/1354869",
            "keyGuide": "创建应用并勾选「流式语音识别大模型」，取得 APP ID 与 AK/SK；新版控制台仅提供 App Key 时需按接入指导适配"
        },
        {
            "id": "tencent",
            "name": "腾讯云",
            "needsKey": true,
            "keyFields": [
                { "key": "appId", "label": "AppID", "placeholder": "例如 1250000000", "type": "text" },
                { "key": "secretId", "label": "SecretId", "placeholder": "AKID...", "type": "text" },
                { "key": "secretKey", "label": "SecretKey", "placeholder": "腾讯云 SecretKey", "type": "password" }
            ],
            "models": [
                { "id": "16k_zh_en", "name": "中英大模型 2.0（会议推荐）" },
                { "id": "16k_zh_large", "name": "大模型增强版" }
            ],
            "console": "https://console.cloud.tencent.com/asr",
            "consoleLabel": "腾讯云 · 语音识别控制台 / API 密钥管理",
            "docUrl": "https://cloud.tencent.com/document/product/1093/131127",
            "keyGuide": "开通语音识别后，在「API 密钥管理」新建密钥，取得 AppID、SecretId、SecretKey；模型名可在主界面手动填写"
        },
        {
            "id": "alicloud",
            "name": "阿里云",
            "needsKey": true,
            "keyFields": [
                { "key": "accessKeyId", "label": "AccessKey ID", "placeholder": "LTAI...", "type": "text" },
                { "key": "accessKeySecret", "label": "AccessKey Secret", "placeholder": "阿里云 AccessKeySecret", "type": "password" },
                { "key": "appKey", "label": "AppKey", "placeholder": "项目 AppKey", "type": "text" }
            ],
            "models": [
                { "id": "domain-meeting-domain", "name": "会议领域（中文）" },
                { "id": "domain-audio-domain", "name": "音视频领域（中文）" },
                { "id": "customer-service-domain", "name": "客服领域（中文）" }
            ],
            "console": "https://nls-portal.console.aliyun.com/",
            "consoleLabel": "阿里云 · 智能语音交互控制台",
            "docUrl": "https://help.aliyun.com/knowledge_detail/72214.html",
            "keyGuide": "开通 NLS → 创建 AccessKey → 创建项目取 Appkey；模型主要由项目绑定，请在「项目功能配置」选择 16 kHz 实时识别模型"
        }
    ])
}

/// 保存云端 ASR 凭证到本地配置（仅存本机，不上传）。
#[tauri::command]
fn save_asr_settings(app: AppHandle, settings: RemoteCredentials) -> Result<(), String> {
    let path = asr_settings_path(&app)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("创建配置目录失败: {e}"))?;
    }
    let json =
        serde_json::to_string_pretty(&settings).map_err(|e| format!("序列化凭证失败: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("写入凭证失败: {e}"))
}

/// 加载本地已保存的云端 ASR 凭证。
#[tauri::command]
fn load_asr_settings(app: AppHandle) -> Result<RemoteCredentials, String> {
    load_remote_credentials(&app)
}

/// 测试云端 ASR 连接（握手验证凭证有效性）。
#[tauri::command]
fn test_asr_connection(app: AppHandle, engine: String, model: String) -> Result<(), String> {
    let kind = parse_engine_kind(&engine)?;
    if !kind.is_remote() {
        return Err("该引擎为本地引擎，无需测试云端连接".to_string());
    }
    let creds = load_remote_credentials(&app)?;
    engine::remote::test_connection(kind, &creds, &model)
}

/// 在系统默认浏览器打开白名单内的帮助文档 / 云控制台链接。
///
/// 只接受预置前缀，避免前端传入任意 URL 造成钓鱼或命令注入风险。
#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    const ALLOWED: &[&str] = &[
        "https://github.com/xiaopengs/moonshine-meeting-assistant",
        "https://console.volcengine.com/",
        "https://www.volcengine.com/",
        "https://console.cloud.tencent.com/",
        "https://cloud.tencent.com/",
        "https://nls-portal.console.aliyun.com/",
        "https://help.aliyun.com/",
    ];
    if !ALLOWED.iter().any(|prefix| url.starts_with(prefix)) {
        return Err("不允许打开该链接（仅支持内置文档与云控制台）".to_string());
    }
    open_in_browser(&url).map_err(|e| format!("打开链接失败: {e}"))
}

/// 跨平台调用系统浏览器（零额外依赖）。
fn open_in_browser(url: &str) -> std::io::Result<()> {
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    let _ = url;
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
    }
    Ok(())
}

#[tauri::command]
fn save_transcript(
    app: AppHandle,
    request: SaveRequest,
) -> Result<Option<String>, String> {
    let file_path = app
        .dialog()
        .file()
        .set_file_name(&request.filename)
        .add_filter("Markdown", &["md"])
        .add_filter("SRT 字幕", &["srt"])
        .add_filter("纯文本", &["txt"])
        .blocking_save_file();

    match file_path {
        Some(FilePath::Path(path)) => {
            std::fs::write(&path, &request.content)
                .map_err(|e| format!("写入失败: {e}"))?;
            Ok(Some(path.to_string_lossy().to_string()))
        }
        Some(FilePath::Url(url)) => Ok(Some(url.to_string())),
        None => Ok(None),
    }
}

#[tauri::command]
fn summarize(
    state: State<'_, AppState>,
    config: SummaryConfig,
) -> Result<String, String> {
    let transcript = {
        let session = state.session.lock().unwrap();
        session
            .segments
            .iter()
            .map(|s| s.text.clone())
            .collect::<Vec<_>>()
            .join("\n")
    };
    if transcript.trim().is_empty() {
        return Err("暂无会议内容可摘要".to_string());
    }
    summary::summarize(&config, &transcript).map_err(|e| e.to_string())
}

#[tauri::command]
fn export_session(state: State<'_, AppState>, format: String) -> Result<String, String> {
    let session = state.session.lock().unwrap();
    let fmt = match format.as_str() {
        "srt" => ExportFormat::Srt,
        "txt" => ExportFormat::Txt,
        _ => ExportFormat::Markdown,
    };
    Ok(export::export(&session, fmt))
}

// ========================
// 转录运行时
// ========================

#[allow(clippy::too_many_arguments)]
fn run_transcription(
    app: &AppHandle,
    config: &StartConfig,
    engine_config: &EngineConfig,
    running: &Arc<AtomicBool>,
    stop_flag: &Arc<AtomicBool>,
    session: &Arc<Mutex<Session>>,
) -> Result<(), String> {
    let emit = |app: &AppHandle, ev: &TranscriptEvent| {
        let _ = app.emit("transcript", ev);
    };
    let log = |app: &AppHandle, msg: String| {
        let _ = app.emit("log", msg);
    };

    // 打开采集流：优先按设备名定位（避免 index 漂移），失败时保留完整配置尝试记录
    let mic = if config.mic_device_index >= 0 {
        Some(
            AudioCapture::open_prefer(
                config.mic_device_name.as_deref(),
                config.mic_device_index as usize,
                CHUNK_SAMPLES,
            )
            .map_err(|e| format!("麦克风打开失败 → {e}"))?,
        )
    } else {
        None
    };

    let loopback = if config.dual_enabled && config.loopback_device_index >= 0 {
        Some(
            AudioCapture::open_prefer(
                config.loopback_device_name.as_deref(),
                config.loopback_device_index as usize,
                CHUNK_SAMPLES,
            )
            .map_err(|e| format!("系统内录打开失败 → {e}"))?,
        )
    } else {
        None
    };

    if mic.is_none() && loopback.is_none() {
        return Err("未选择任何音频设备".to_string());
    }

    // 把实际打开的设备与命中配置写进日志，便于真机核对有没有打开错设备
    if let Some(m) = &mic {
        log(app, format!("麦克风已打开: {} · {}", m.device_name(), m.config_desc()));
    }
    if let Some(l) = &loopback {
        log(app, format!("系统内录已打开: {} · {}", l.device_name(), l.config_desc()));
    }

    // 创建引擎（每个流一个独立实例，避免状态混淆）
    let mut mic_engine = mic
        .as_ref()
        .map(|_| engine::create_engine(engine_config, Source::Mic))
        .transpose()
        .map_err(|e| e.to_string())?;

    let mut loop_engine = if loopback.is_some() {
        Some(
            engine::create_engine(engine_config, Source::Loopback).map_err(|e| e.to_string())?,
        )
    } else {
        None
    };

    let mut vad = EnergyVad::default();
    let diarizer = DualChannelDiarizer;

    {
        session.lock().unwrap().start();
    }
    log(app, format!("转写启动 · 引擎 {}", engine_config.kind.label()));
    let _ = app.emit("status", "running");

    let mut last_volume_emit = Instant::now();

    while !stop_flag.load(Ordering::SeqCst) {
        let mut any = false;

        // 麦克风流
        if let (Some(cap), Some(eng)) = (mic.as_ref(), mic_engine.as_mut()) {
            while let Some(samples) = cap.try_recv() {
                any = true;
                emit_volume(app, &samples, &mut last_volume_emit);
                if !config.vad_enabled || vad.is_speech(&samples) {
                    if let Ok(segs) = eng.feed(&samples, 16_000) {
                        push_segments(app, &emit, &diarizer, session, segs);
                    }
                }
            }
        }

        // 系统内录流
        if let (Some(cap), Some(eng)) = (loopback.as_ref(), loop_engine.as_mut()) {
            while let Some(samples) = cap.try_recv() {
                any = true;
                emit_volume(app, &samples, &mut last_volume_emit);
                if !config.vad_enabled || vad.is_speech(&samples) {
                    if let Ok(segs) = eng.feed(&samples, 16_000) {
                        push_segments(app, &emit, &diarizer, session, segs);
                    }
                }
            }
        }

        if !any {
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    // flush 引擎最终文本
    if let Some(eng) = mic_engine.as_mut() {
        if let Ok(segs) = eng.flush() {
            push_segments(app, &emit, &diarizer, session, segs);
        }
    }
    if let Some(eng) = loop_engine.as_mut() {
        if let Ok(segs) = eng.flush() {
            push_segments(app, &emit, &diarizer, session, segs);
        }
    }

    log(app, "转写已停止".to_string());
    let _ = app.emit("status", "stopped");
    running.store(false, Ordering::SeqCst);
    Ok(())
}

fn push_segments<F>(
    app: &AppHandle,
    emit: &F,
    diarizer: &DualChannelDiarizer,
    session: &Arc<Mutex<Session>>,
    segments: Vec<Segment>,
) where
    F: Fn(&AppHandle, &TranscriptEvent),
{
    for mut seg in segments {
        diarizer.annotate(&mut seg);
        let ev = TranscriptEvent {
            source: seg.source.as_str().to_string(),
            speaker: seg.speaker.clone(),
            text: seg.text.clone(),
            start: seg.start,
            end: seg.end,
            is_final: seg.is_final,
            ts: now_secs(),
        };
        emit(app, &ev);
        session.lock().unwrap().push(seg);
    }
}

fn emit_volume(app: &AppHandle, samples: &[f32], last: &mut Instant) {
    // 节流：每 50ms 最多发一次电平，用于前端波形可视化。
    if last.elapsed().as_millis() < 50 {
        return;
    }
    *last = Instant::now();
    let vol = rms(samples);
    let _ = app.emit(
        "volume",
        serde_json::json!({ "volume": vol }),
    );
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

// ========================
// 入口
// ========================

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            list_devices,
            probe_audio_devices,
            get_version,
            start_transcription,
            stop_transcription,
            save_transcript,
            summarize,
            export_session,
            get_asr_providers,
            save_asr_settings,
            load_asr_settings,
            test_asr_connection,
            open_external_url,
        ])
        .run(tauri::generate_context!())
        .expect("Moonshine 会议助手启动失败");
}
