//! 多引擎可插拔转写抽象层。
//!
//! 定义统一的 [`TranscriptionEngine`] trait 与 [`Segment`] 数据结构，
//! 通过 Python 子进程（`python/engine_service.py`）驱动各 ASR 后端：
//! - [`FasterWhisperEngine`]：faster-whisper（CTranslate2），中文能力强，默认引擎
//! - [`MoonshineEngine`]：moonshine-voice，英文流式
//! - [`FunasrEngine`]：FunASR/Paraformer，中文流式专用

mod funasr;
mod moonshine;
mod whisper;

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use funasr::FunasrEngine;
pub use moonshine::MoonshineEngine;
pub use whisper::FasterWhisperEngine;

/// 音频来源（双路物理隔离：麦克风 = 本端，内录 = 远端）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Mic,
    Loopback,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::Mic => "mic",
            Source::Loopback => "loopback",
        }
    }

    /// 默认说话人标签（双路物理隔离场景）。
    pub fn default_speaker(&self) -> &'static str {
        match self {
            Source::Mic => "本端",
            Source::Loopback => "远端",
        }
    }
}

/// 一段转写结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Segment {
    pub source: Source,
    pub speaker: Option<String>,
    pub text: String,
    pub start: f32,
    pub end: f32,
    #[serde(default)]
    pub is_final: bool,
}

impl Segment {
    pub fn new(source: Source, text: String) -> Self {
        Self {
            source,
            speaker: Some(source.default_speaker().to_string()),
            text,
            start: 0.0,
            end: 0.0,
            is_final: true,
        }
    }
}

/// 转写引擎类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineKind {
    FasterWhisper,
    Moonshine,
    Funasr,
}

impl EngineKind {
    /// 传给 Python 服务的引擎标识。
    pub fn as_str(&self) -> &'static str {
        match self {
            EngineKind::FasterWhisper => "faster_whisper",
            EngineKind::Moonshine => "moonshine",
            EngineKind::Funasr => "funasr",
        }
    }

    /// 用户可读名称。
    pub fn label(&self) -> &'static str {
        match self {
            EngineKind::FasterWhisper => "faster-whisper（中文优先）",
            EngineKind::Moonshine => "moonshine（英文流式）",
            EngineKind::Funasr => "FunASR / Paraformer（中文流式）",
        }
    }
}

/// 引擎配置。
#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub kind: EngineKind,
    pub model: String,
    pub language: Option<String>,
    pub python: String,
    pub script: PathBuf,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            kind: EngineKind::FasterWhisper,
            model: "large-v3-turbo".to_string(),
            language: Some("zh".to_string()),
            python: if cfg!(windows) { "python".into() } else { "python3".into() },
            script: PathBuf::from("python/engine_service.py"),
        }
    }
}

/// 引擎错误。
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("引擎启动失败: {0}")]
    Spawn(String),
    #[error("引擎 I/O 失败: {0}")]
    Io(#[from] std::io::Error),
    #[error("引擎返回错误: {0}")]
    Engine(String),
    #[error("JSON 序列化失败: {0}")]
    Serde(#[from] serde_json::Error),
}

/// 统一转写引擎 trait。
pub trait TranscriptionEngine: Send {
    fn id(&self) -> &'static str;

    /// 送入音频 chunk，返回本批新增/修正的文本片段。
    fn feed(&mut self, samples: &[f32], sample_rate: u32) -> Result<Vec<Segment>, EngineError>;

    /// 结束一段话，返回最终文本片段。
    fn flush(&mut self) -> Result<Vec<Segment>, EngineError>;
}

/// 按配置创建引擎实例（每个音频流一个实例，双路时各建一个）。
pub fn create_engine(
    config: &EngineConfig,
    source: Source,
) -> Result<Box<dyn TranscriptionEngine>, EngineError> {
    match config.kind {
        EngineKind::FasterWhisper => Ok(Box::new(FasterWhisperEngine::new(config, source)?)),
        EngineKind::Moonshine => Ok(Box::new(MoonshineEngine::new(config, source)?)),
        EngineKind::Funasr => Ok(Box::new(FunasrEngine::new(config, source)?)),
    }
}

// ========================
// Python 子进程客户端（各后端共用）
// ========================

enum EngineEvent {
    Log(String),
    Segment(Segment),
    Error(String),
}

pub(crate) struct PythonEngineClient {
    stdin: ChildStdin,
    rx: Receiver<EngineEvent>,
    child: Child,
}

impl PythonEngineClient {
    pub fn spawn(config: &EngineConfig) -> Result<Self, EngineError> {
        let mut cmd = Command::new(&config.python);
        cmd.arg(&config.script)
            .arg("--engine")
            .arg(config.kind.as_str())
            .arg("--model")
            .arg(&config.model);
        if let Some(lang) = &config.language {
            cmd.arg("--language").arg(lang);
        }

        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| EngineError::Spawn(e.to_string()))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| EngineError::Spawn("无法获取子进程 stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| EngineError::Spawn("无法获取子进程 stdout".to_string()))?;

        let (tx, rx) = channel::<EngineEvent>();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let ev = match serde_json::from_str::<serde_json::Value>(line) {
                    Ok(v) => match parse_event(&v) {
                        Some(ev) => ev,
                        None => continue,
                    },
                    Err(_) => EngineEvent::Log(line.to_string()),
                };
                if tx.send(ev).is_err() {
                    break;
                }
            }
        });

        Ok(Self { stdin, rx, child })
    }

    fn send(&mut self, cmd: serde_json::Value) -> Result<(), EngineError> {
        let mut line = serde_json::to_string(&cmd)?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes())?;
        self.stdin.flush()?;
        Ok(())
    }

    pub fn load(&mut self) -> Result<(), EngineError> {
        self.send(serde_json::json!({ "cmd": "load" }))
    }

    pub fn feed(&mut self, samples: &[f32], sample_rate: u32) -> Result<(), EngineError> {
        self.send(serde_json::json!({
            "cmd": "feed",
            "samples": encode_f32_le(samples),
            "rate": sample_rate,
        }))
    }

    pub fn flush(&mut self) -> Result<(), EngineError> {
        self.send(serde_json::json!({ "cmd": "flush" }))
    }

    /// 非阻塞地取回所有待处理事件，仅返回转写片段。
    pub fn drain(&mut self, source: Source) -> Vec<Segment> {
        let mut out = Vec::new();
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                EngineEvent::Segment(mut s) => {
                    s.source = source;
                    out.push(s);
                }
                EngineEvent::Log(m) => log::info!("[engine] {}", m),
                EngineEvent::Error(e) => log::error!("[engine] {}", e),
            }
        }
        out
    }
}

impl Drop for PythonEngineClient {
    fn drop(&mut self) {
        let _ = self.stdin.write_all(b"{\"cmd\":\"stop\"}\n");
        let _ = self.stdin.flush();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn parse_event(v: &serde_json::Value) -> Option<EngineEvent> {
    if let Some(msg) = v.get("log").and_then(|x| x.as_str()) {
        return Some(EngineEvent::Log(msg.to_string()));
    }
    if let Some(err) = v.get("error").and_then(|x| x.as_str()) {
        return Some(EngineEvent::Error(err.to_string()));
    }
    if v.get("type").and_then(|x| x.as_str()) == Some("segment") {
        return Some(EngineEvent::Segment(Segment {
            source: Source::Mic, // 由调用方覆盖为实际来源
            speaker: None,
            text: v
                .get("text")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            start: v.get("start").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
            end: v.get("end").and_then(|x| x.as_f64()).unwrap_or(0.0) as f32,
            is_final: v.get("final").and_then(|x| x.as_bool()).unwrap_or(true),
        }));
    }
    None
}

fn encode_f32_le(samples: &[f32]) -> String {
    let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    B64.encode(&bytes)
}
