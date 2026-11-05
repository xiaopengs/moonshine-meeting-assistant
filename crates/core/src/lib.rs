//! moonshine-core — Moonshine 会议助手核心库
//!
//! UI 无关的核心能力，供 Tauri 壳（或其他宿主）调用：
//! - [`audio`]：cpal 跨平台双路音频采集
//! - [`vad`]：能量 VAD 静音检测
//! - [`engine`]：多引擎可插拔转写抽象（faster-whisper / moonshine / funasr）
//! - [`diarization`]：说话人归属（双路物理隔离）
//! - [`summary`]：OpenAI 兼容会议纪要
//! - [`export`]：MD / SRT / TXT 导出
//! - [`session`]：会话状态聚合

pub mod audio;
pub mod diarization;
pub mod engine;
pub mod export;
pub mod session;
pub mod summary;
pub mod vad;

pub use engine::{EngineConfig, EngineKind, Segment, Source, TranscriptionEngine};
pub use session::Session;
