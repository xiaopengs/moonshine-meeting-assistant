//! 跨平台音频采集（cpal）。
//!
//! 枚举输入设备、识别内录设备，并提供 [`AudioCapture`] 双路采集。

mod capture;

pub use capture::AudioCapture;

use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;

/// 音频输入设备描述。
#[derive(Debug, Clone, Serialize)]
pub struct AudioDevice {
    pub index: usize,
    pub name: String,
    pub channels: u16,
    pub default_rate: u32,
    pub is_loopback: bool,
}

/// 枚举所有输入设备（麦克风 + 系统内录）。
pub fn list_input_devices() -> Result<Vec<AudioDevice>, String> {
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .map_err(|e| format!("无法枚举输入设备: {e}"))?;

    let mut out = Vec::new();
    for (index, device) in devices.enumerate() {
        let name = device.name().unwrap_or_else(|_| format!("音频设备 {index}"));
        let (channels, default_rate) = device
            .default_input_config()
            .map(|c| (c.channels(), c.sample_rate().0))
            .unwrap_or((0, 0));
        out.push(AudioDevice {
            index,
            is_loopback: is_loopback_name(&name),
            name,
            channels,
            default_rate,
        });
    }
    Ok(out)
}

/// 依据设备名识别内录（Loopback）设备。
///
/// 覆盖 Windows（Stereo Mix / WASAPI Loopback）、macOS（BlackHole / Soundflower）、
/// Linux（PulseAudio/PipeWire monitor）等常见命名变体。
pub fn is_loopback_name(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("loopback")
        || n.contains("stereo mix")
        || n.contains("立体声混音")
        || n.contains("what u hear")
        || n.contains("what i hear")
        || n.contains("blackhole")
        || n.contains("soundflower")
        || n.contains("virtual audio cable")
        || n.contains("vac ")
        || n.contains("monitor")
}
