//! 跨平台音频采集（cpal）。
//!
//! 枚举输入设备、识别内录设备，并提供 [`AudioCapture`] 双路采集与设备诊断。

mod capture;

pub use capture::{AudioCapture, ProbeOutcome, TARGET_SAMPLE_RATE};

use cpal::traits::{DeviceTrait, HostTrait};
use serde::Serialize;

/// 音频输入设备描述。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDevice {
    pub index: usize,
    pub name: String,
    pub channels: u16,
    pub default_rate: u32,
    /// 明确的内录设备（强关键词命中）。
    pub is_loopback: bool,
    /// 疑似内录设备（弱关键词命中，仅用于 UI 排序与提示，不做硬性过滤）。
    pub loopback_hint: bool,
}

/// 单个设备的诊断结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceProbe {
    pub index: usize,
    pub name: String,
    pub is_loopback: bool,
    pub loopback_hint: bool,
    /// 是否成功打开输入流。
    pub ok: bool,
    /// 命中的配置描述（成功时）。
    pub config: Option<String>,
    /// 失败原因摘要。
    pub error: Option<String>,
    /// 各次配置尝试的明细（含原始系统错误）。
    pub attempts: Vec<String>,
}

/// 当前音频宿主（Alsa / Wasapi / CoreAudio 等），诊断时展示。
pub fn host_name() -> String {
    format!("{:?}", cpal::default_host().id())
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
            loopback_hint: is_loopback_candidate(&name),
            name,
            channels,
            default_rate,
        });
    }
    Ok(out)
}

/// 逐个设备真实尝试打开输入流（不播放），返回可打开性与详细错误。
///
/// 用于「打开内录失败」时定位：究竟是设备不存在、采样格式不支持、缓冲不被接受，
/// 还是权限被拒。前端「音频诊断」与独立 `audio_probe.exe` 都调用它。
pub fn probe_devices() -> Result<Vec<DeviceProbe>, String> {
    let host = cpal::default_host();
    let devices: Vec<cpal::Device> = host
        .input_devices()
        .map_err(|e| format!("无法枚举输入设备: {e}"))?
        .collect();

    let mut out = Vec::new();
    for (index, device) in devices.iter().enumerate() {
        let name = device.name().unwrap_or_else(|_| format!("音频设备 {index}"));
        let result = capture::probe_open(device, 2560);
        out.push(DeviceProbe {
            index,
            is_loopback: is_loopback_name(&name),
            loopback_hint: is_loopback_candidate(&name),
            name,
            ok: result.ok,
            config: result.config,
            error: if result.ok {
                None
            } else {
                Some(format!(
                    "共 {} 种配置均失败，末次错误：{}",
                    result.attempts.len(),
                    result.attempts.last().cloned().unwrap_or_default()
                ))
            },
            attempts: result.attempts,
        });
    }
    Ok(out)
}

/// 明确的内录设备判定（强关键词）。
///
/// 覆盖 Windows（Stereo Mix / WASAPI Loopback / 虚拟声卡）、macOS（BlackHole /
/// Soundflower）、Linux（PulseAudio / PipeWire monitor）等常见命名变体。
pub fn is_loopback_name(name: &str) -> bool {
    let n = name.to_lowercase();
    const STRONG: &[&str] = &[
        "loopback",
        "stereo mix",
        "stereomix",
        "立体声混音",
        "what u hear",
        "what i hear",
        "wave out mix",
        "mixage stéréo",
        "blackhole",
        "soundflower",
        "virtual audio cable",
        "vac ",
        "cable output",
        "cable input",
        "vb-audio",
        "voicemeeter",
        "monitor of",
        ".monitor",
        "monitor",
        "系统内录",
        "系统声音",
        "混音输出",
    ];
    STRONG.iter().any(|k| n.contains(k))
}

/// 疑似内录设备（弱关键词）：仅用于 UI 排序与提示。
///
/// Windows WASAPI 常把 loopback 设备命名为「扬声器 (Realtek…)」/「Speakers」，
/// cpal 通过 ALSA pulse 插件在 Linux 上枚举出的名字则是 `pulse` / `default`，
/// 这些都不含强关键词，但用户仍可能需要手动选作内录源。
pub fn is_loopback_candidate(name: &str) -> bool {
    if is_loopback_name(name) {
        return true;
    }
    let n = name.to_lowercase();
    const WEAK: &[&str] = &[
        "speakers",
        "speaker",
        "扬声器",
        "喇叭",
        "耳机",
        "headphone",
        "headset",
        "pulse",
        "default",
        "render",
        "输出",
        "output",
        "virtual",
        "cable",
    ];
    WEAK.iter().any(|k| n.contains(k))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strong_loopback_names() {
        for n in [
            "立体声混音 (Realtek High Definition Audio)",
            "Stereo Mix (Realtek)",
            "WASAPI Loopback",
            "Monitor of Built-in Audio Analog Stereo",
            "alsa_output.pci-0000_00_1f.3.analog-stereo.monitor",
            "BlackHole 2ch",
            "Soundflower (2ch)",
            "CABLE Output (VB-Audio Virtual Cable)",
            "系统声音",
        ] {
            assert!(is_loopback_name(n), "应识别为内录: {n}");
            assert!(is_loopback_candidate(n), "同时应为候选: {n}");
        }
    }

    #[test]
    fn weak_candidates_are_not_strong() {
        // Windows WASAPI loopback / PulseAudio 插件常见名称
        for n in [
            "扬声器 (Realtek High Definition Audio)",
            "Speakers (Realtek HD Audio)",
            "pulse",
            "default",
            "Headphones (USB Audio)",
        ] {
            assert!(!is_loopback_name(n), "不应作为强判定: {n}");
            assert!(is_loopback_candidate(n), "应作为候选: {n}");
        }
    }

    #[test]
    fn normal_mics_are_not_loopback() {
        for n in [
            "麦克风 (Realtek High Definition Audio)",
            "Microphone (USB)",
            "Built-in Microphone",
            "默认麦克风",
        ] {
            assert!(!is_loopback_name(n), "误判为内录: {n}");
            assert!(!is_loopback_candidate(n), "误判为候选: {n}");
        }
    }

    #[test]
    fn case_insensitive() {
        assert!(is_loopback_name("STEREO MIX"));
        assert!(is_loopback_candidate("Speakers (Realtek)"));
    }
}
