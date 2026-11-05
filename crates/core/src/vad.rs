//! VAD（Voice Activity Detection）静音检测。
//!
//! 默认使用零依赖的能量检测（RMS 阈值），保留神经网络 VAD 的扩展点。

/// VAD 检测器 trait，便于后续替换为 silero-vad 等方案。
pub trait Vad: Send {
    /// 判定当前音频块是否为语音。
    fn is_speech(&mut self, samples: &[f32]) -> bool;
}

/// 能量 VAD：RMS 超过阈值即判定为语音。
#[derive(Debug, Clone)]
pub struct EnergyVad {
    threshold: f32,
}

impl EnergyVad {
    pub fn new(threshold: f32) -> Self {
        Self { threshold }
    }
}

impl Default for EnergyVad {
    fn default() -> Self {
        Self::new(0.01)
    }
}

impl Vad for EnergyVad {
    fn is_speech(&mut self, samples: &[f32]) -> bool {
        rms(samples) > self.threshold
    }
}

/// 计算音频块 RMS 能量（0.0..1.0 归一化区间附近），可用于波形可视化。
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}
