//! 会话状态聚合：转写片段的统一存储与统计。

use std::time::Instant;

use serde::Serialize;

use crate::engine::{Segment, Source};

/// 一次会议会话的聚合状态。
#[derive(Debug, Default)]
pub struct Session {
    pub segments: Vec<Segment>,
    pub started_at: Option<Instant>,
    mic_chars: usize,
    loopback_chars: usize,
}

/// 会话统计快照（供 UI 展示）。
#[derive(Debug, Clone, Serialize)]
pub struct SessionStats {
    pub segments: usize,
    pub mic_chars: usize,
    pub loopback_chars: usize,
    pub total_chars: usize,
    pub mic_ratio: f32,
    pub loopback_ratio: f32,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(&mut self) {
        self.started_at = Some(Instant::now());
    }

    pub fn push(&mut self, mut segment: Segment) {
        match segment.source {
            Source::Mic => self.mic_chars += segment.text.chars().count(),
            Source::Loopback => self.loopback_chars += segment.text.chars().count(),
        }
        if segment.speaker.is_none() {
            segment.speaker = Some(segment.source.default_speaker().to_string());
        }
        self.segments.push(segment);
    }

    pub fn elapsed_secs(&self) -> f64 {
        self.started_at
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0)
    }

    pub fn total_chars(&self) -> usize {
        self.mic_chars + self.loopback_chars
    }

    pub fn stats(&self) -> SessionStats {
        let total = self.mic_chars + self.loopback_chars;
        let (mic_ratio, loopback_ratio) = if total == 0 {
            (0.5, 0.5)
        } else {
            (
                self.mic_chars as f32 / total as f32,
                self.loopback_chars as f32 / total as f32,
            )
        };
        SessionStats {
            segments: self.segments.len(),
            mic_chars: self.mic_chars,
            loopback_chars: self.loopback_chars,
            total_chars: total,
            mic_ratio,
            loopback_ratio,
        }
    }

    pub fn clear(&mut self) {
        self.segments.clear();
        self.mic_chars = 0;
        self.loopback_chars = 0;
        self.started_at = None;
    }
}
