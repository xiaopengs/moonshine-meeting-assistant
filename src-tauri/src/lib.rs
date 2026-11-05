//! Tauri 2 应用壳：命令注册、事件派发、转录运行时。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use moonshine_core::audio::{list_input_devices, AudioCapture};
use moonshine_core::diarization::{Diarizer, DualChannelDiarizer};
use moonshine_core::engine::{self, EngineConfig, EngineKind, Segment, Source};
use moonshine_core::export::{self, ExportFormat};
use moonshine_core::session::Session;
use moonshine_core::summary::{self, SummaryConfig};
use moonshine_core::vad::{rms, EnergyVad, Vad};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
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
        other => Err(format!("未知转写引擎: {other}")),
    }
}

fn default_engine_script() -> PathBuf {
    if let Ok(p) = std::env::var("MOONSHINE_ENGINE_SCRIPT") {
        return PathBuf::from(p);
    }
    PathBuf::from("python/engine_service.py")
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
    let engine_config = EngineConfig {
        kind,
        model: config.model.clone(),
        language: if config.language.is_empty() {
            None
        } else {
            Some(config.language.clone())
        },
        script: default_engine_script(),
        ..Default::default()
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

    // 打开采集流
    let mic = if config.mic_device_index >= 0 {
        Some(
            AudioCapture::open(config.mic_device_index as usize, CHUNK_SAMPLES)
                .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };

    let loopback = if config.dual_enabled && config.loopback_device_index >= 0 {
        Some(
            AudioCapture::open(config.loopback_device_index as usize, CHUNK_SAMPLES)
                .map_err(|e| e.to_string())?,
        )
    } else {
        None
    };

    if mic.is_none() && loopback.is_none() {
        return Err("未选择任何音频设备".to_string());
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
            get_version,
            start_transcription,
            stop_transcription,
            save_transcript,
            summarize,
            export_session,
        ])
        .run(tauri::generate_context!())
        .expect("Moonshine 会议助手启动失败");
}
