//! 单路音频采集封装：统一输出 16kHz 单声道 f32 块。
//!
//! 设计要点（针对「打开内录失败」）：
//! - **多采样格式**：真实设备（尤其 Windows WASAPI loopback）可能只暴露 I16/F32 等
//!   单一采样格式，仅用 f32 建流会全部失败。这里按设备声明的格式逐一尝试。
//! - **多配置探测**：16kHz/1ch 优先，失败后按设备默认配置、supported_input_configs
//!   范围、常见采样率逐级回退，且 `BufferSize::Default` 优先于 `Fixed`。
//! - **按名定位**：优先用设备名匹配（枚举与打开之间设备列表可能变化），index 兜底。
//! - **错误可诊断**：保留每一次失败的原始 OS 错误，而不是吞掉只报一句笼统失败。

use std::collections::HashSet;
use std::sync::mpsc::{channel, Receiver, Sender};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, StreamConfig};
use thiserror::Error;

/// 输出采样率（moonshine / faster-whisper / 云 ASR 约定）。
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("音频宿主错误: {0}")]
    Host(String),
    #[error("未找到设备 index={0}；当前可用设备: {1}")]
    DeviceNotFound(usize, String),
    #[error("设备 [{0}] 无法打开输入流（共尝试 {2} 种配置）: {1}")]
    Stream(String, String, usize),
    #[error("设备 [{0}] 采集流启动失败: {1}")]
    Play(String, String),
}

/// 诊断结果：某个设备能否打开、命中的配置、以及各次尝试记录。
#[derive(Debug, Clone)]
pub struct ProbeOutcome {
    pub ok: bool,
    pub config: Option<String>,
    pub attempts: Vec<String>,
}

/// 一个音频输入流的采集封装。
pub struct AudioCapture {
    _stream: cpal::Stream,
    rx: Receiver<Vec<f32>>,
    device_name: String,
    config_desc: String,
}

impl AudioCapture {
    /// 兼容旧调用：仅按 index 打开。
    pub fn open(index: usize, chunk_samples: usize) -> Result<Self, AudioError> {
        Self::open_prefer(None, index, chunk_samples)
    }

    /// 打开指定设备。**优先按设备名定位**，找不到时退回 index。
    ///
    /// 优先级的意义：前端枚举设备与用户点击「开始录制」之间可能插入蓝牙耳机、
    /// 系统切换默认设备等，导致 `input_devices().nth(index)` 指向错误设备。
    pub fn open_prefer(
        name: Option<&str>,
        index: usize,
        chunk_samples: usize,
    ) -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let devices: Vec<cpal::Device> = host
            .input_devices()
            .map_err(|e| AudioError::Host(e.to_string()))?
            .collect();

        let names: Vec<String> = devices
            .iter()
            .map(|d| d.name().unwrap_or_else(|_| "<无名设备>".into()))
            .collect();

        let idx = locate(&devices, name, index)
            .ok_or_else(|| AudioError::DeviceNotFound(index, names.join(" / ")))?;
        let device = &devices[idx];
        let dev_name = names.get(idx).cloned().unwrap_or_else(|| format!("设备 {idx}"));

        let (tx, rx) = channel::<Vec<f32>>();
        let (stream, desc, attempts) =
            build_stream(device, chunk_samples, tx, &dev_name).map_err(|failed| {
                AudioError::Stream(dev_name.clone(), summarize_attempts(&failed), failed.len())
            })?;

        stream
            .play()
            .map_err(|e| AudioError::Play(dev_name.clone(), e.to_string()))?;

        let _ = attempts; // 成功时尝试记录仅用于诊断，不参与运行时
        Ok(Self {
            _stream: stream,
            rx,
            device_name: dev_name,
            config_desc: desc,
        })
    }

    /// 实际打开的设备名（用于日志核对，确认没有打开错设备）。
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// 实际命中的流配置描述（格式 / 采样率 / 声道 / 缓冲）。
    pub fn config_desc(&self) -> &str {
        &self.config_desc
    }

    /// 非阻塞取回一个音频块。
    pub fn try_recv(&self) -> Option<Vec<f32>> {
        self.rx.try_recv().ok()
    }
}

/// 诊断用：对某个设备尝试所有候选配置，但不真正启动采集。
pub fn probe_open(device: &cpal::Device, chunk_samples: usize) -> ProbeOutcome {
    let name = device.name().unwrap_or_else(|_| "<无名设备>".into());
    let (tx, _rx) = channel::<Vec<f32>>();
    match build_stream(device, chunk_samples, tx, &name) {
        Ok((_stream, desc, attempts)) => ProbeOutcome {
            ok: true,
            config: Some(desc),
            attempts,
        },
        Err(attempts) => ProbeOutcome {
            ok: false,
            config: None,
            attempts,
        },
    }
}

// ========================
// 设备定位
// ========================

fn locate(devices: &[cpal::Device], name: Option<&str>, index: usize) -> Option<usize> {
    if let Some(want) = name.map(str::trim).filter(|s| !s.is_empty()) {
        // 精确匹配
        if let Some(i) = devices
            .iter()
            .position(|d| d.name().map(|n| n.trim() == want).unwrap_or(false))
        {
            return Some(i);
        }
        // 大小写不敏感匹配（跨进程/系统区域设置时名称可能变化）
        let lower = want.to_lowercase();
        if let Some(i) = devices.iter().position(|d| {
            d.name()
                .map(|n| n.trim().to_lowercase() == lower)
                .unwrap_or(false)
        }) {
            return Some(i);
        }
    }
    devices.get(index).map(|_| index)
}

// ========================
// 多配置 / 多格式建流
// ========================

fn add_config(
    cfgs: &mut Vec<StreamConfig>,
    seen: &mut HashSet<(u16, u32, String)>,
    channels: u16,
    rate: u32,
    buffer_size: BufferSize,
) {
    let key = (
        channels,
        rate,
        match &buffer_size {
            BufferSize::Fixed(n) => format!("F{n}"),
            _ => "D".to_string(),
        },
    );
    if seen.insert(key) {
        cfgs.push(StreamConfig {
            channels,
            sample_rate: cpal::SampleRate(rate),
            buffer_size,
        });
    }
}

fn candidate_configs(device: &cpal::Device, chunk_samples: usize) -> Vec<StreamConfig> {
    let mut cfgs = Vec::new();
    let mut seen = HashSet::new();
    let chunk = chunk_samples.clamp(256, 8192) as u32;
    let default = device.default_input_config().ok();

    // ① 最理想：直接 16kHz 单声道，无需重采样
    add_config(&mut cfgs, &mut seen, 1, TARGET_SAMPLE_RATE, BufferSize::Default);
    add_config(&mut cfgs, &mut seen, 1, TARGET_SAMPLE_RATE, BufferSize::Fixed(chunk));

    // ② 设备默认配置（WASAPI/CoreAudio 最常见可用组合）
    if let Some(c) = &default {
        let rate = c.sample_rate().0;
        add_config(&mut cfgs, &mut seen, c.channels(), rate, BufferSize::Default);
        add_config(&mut cfgs, &mut seen, c.channels(), rate, BufferSize::Fixed(chunk));
        add_config(&mut cfgs, &mut seen, 1, rate, BufferSize::Default);
    }

    // ③ 设备显式声明支持的配置范围
    if let Ok(ranges) = device.supported_input_configs() {
        for r in ranges {
            let lo = r.min_sample_rate().0;
            let hi = r.max_sample_rate().0;
            for want in [TARGET_SAMPLE_RATE, 48_000, 44_100] {
                let rate = want.clamp(lo.min(hi), hi.max(lo));
                add_config(&mut cfgs, &mut seen, r.channels(), rate, BufferSize::Default);
                add_config(&mut cfgs, &mut seen, 1, rate, BufferSize::Default);
            }
        }
    }

    // ④ 常见兜底组合
    for rate in [48_000, 44_100, 32_000, 16_000] {
        add_config(&mut cfgs, &mut seen, 1, rate, BufferSize::Default);
        add_config(&mut cfgs, &mut seen, 2, rate, BufferSize::Default);
    }

    cfgs
}

fn candidate_formats(device: &cpal::Device) -> Vec<cpal::SampleFormat> {
    use cpal::SampleFormat as F;
    let mut out = Vec::new();
    if let Ok(c) = device.default_input_config() {
        out.push(c.sample_format());
    }
    for f in [F::F32, F::I16, F::U16, F::I32, F::F64] {
        if !out.contains(&f) {
            out.push(f);
        }
    }
    out
}

/// 把多种采样格式统一转换为 f32（手写避免额外 trait 依赖，便于交叉编译）。
trait ToF32: Copy {
    fn to_f32(self) -> f32;
}
impl ToF32 for f32 {
    fn to_f32(self) -> f32 {
        self
    }
}
impl ToF32 for f64 {
    fn to_f32(self) -> f32 {
        self as f32
    }
}
impl ToF32 for i16 {
    fn to_f32(self) -> f32 {
        self as f32 / 32_768.0
    }
}
impl ToF32 for u16 {
    fn to_f32(self) -> f32 {
        (self as f32 - 32_768.0) / 32_768.0
    }
}
impl ToF32 for i32 {
    fn to_f32(self) -> f32 {
        self as f32 / 2_147_483_648.0
    }
}
impl ToF32 for u8 {
    fn to_f32(self) -> f32 {
        (self as f32 - 128.0) / 128.0
    }
}
impl ToF32 for i8 {
    fn to_f32(self) -> f32 {
        self as f32 / 128.0
    }
}

fn build_stream(
    device: &cpal::Device,
    chunk_samples: usize,
    tx: Sender<Vec<f32>>,
    dev_name: &str,
) -> Result<(cpal::Stream, String, Vec<String>), Vec<String>> {
    let configs = candidate_configs(device, chunk_samples);
    let formats = candidate_formats(device);
    let mut attempts = Vec::new();
    let mut total = 0usize;

    for cfg in &configs {
        for fmt in &formats {
            total += 1;
            let label = format!(
                "{}ch/{}Hz/{}/{:?}",
                cfg.channels,
                cfg.sample_rate.0,
                match &cfg.buffer_size {
                    BufferSize::Fixed(n) => format!("Fixed({n})"),
                    _ => "Default".to_string(),
                },
                fmt
            );
            let result = try_format(device, cfg, *fmt, tx.clone());
            match result {
                Ok(stream) => {
                    let desc = format!(
                        "{label} → 内部混单/重采样为 16kHz 单声道 f32（设备 {dev_name}）"
                    );
                    return Ok((stream, desc, attempts));
                }
                Err(e) => {
                    // 只保留前 24 条明细，避免错误信息过长；总数单独汇总
                    if attempts.len() < 24 {
                        attempts.push(format!("{label}: {e}"));
                    }
                }
            }
        }
    }
    if attempts.len() < total {
        attempts.push(format!("……其余 {} 种组合同样失败", total - attempts.len()));
    }
    Err(attempts)
}

fn try_format(
    device: &cpal::Device,
    cfg: &StreamConfig,
    fmt: cpal::SampleFormat,
    tx: Sender<Vec<f32>>,
) -> Result<cpal::Stream, String> {
    let channels = cfg.channels as usize;
    let src_rate = cfg.sample_rate.0;

    macro_rules! build {
        ($t:ty) => {{
            let tx = tx.clone();
            device.build_input_stream::<$t, _, _>(
                cfg,
                move |data: &[$t], _| {
                    let mono = to_mono(data, channels);
                    let out = resample_linear(&mono, src_rate, TARGET_SAMPLE_RATE);
                    let _ = tx.send(out);
                },
                err_fn,
                None,
            )
        }};
    }

    let r = match fmt {
        cpal::SampleFormat::F32 => build!(f32),
        cpal::SampleFormat::I16 => build!(i16),
        cpal::SampleFormat::U16 => build!(u16),
        cpal::SampleFormat::I32 => build!(i32),
        cpal::SampleFormat::F64 => build!(f64),
        cpal::SampleFormat::U8 => build!(u8),
        cpal::SampleFormat::I8 => build!(i8),
        other => return Err(format!("暂不支持的采样格式 {other:?}")),
    };
    r.map_err(|e| e.to_string())
}

fn summarize_attempts(failed: &[String]) -> String {
    failed.join("\n    · ")
}

fn err_fn(err: cpal::StreamError) {
    log::warn!("[audio] 采集流错误: {err}");
}

/// interleaved 多声道 → 单声道（逐样本平均），同时转换为 f32。
fn to_mono<T: ToF32>(data: &[T], channels: usize) -> Vec<f32> {
    if data.is_empty() {
        return Vec::new();
    }
    if channels <= 1 {
        return data.iter().map(|&s| s.to_f32()).collect();
    }
    let frames = data.len() / channels;
    (0..frames)
        .map(|i| {
            let sum: f32 = (0..channels).map(|c| data[i * channels + c].to_f32()).sum();
            sum / channels as f32
        })
        .collect()
}

/// 线性插值重采样（含边界保护，空输入/极短输入不会 panic）。
fn resample_linear(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == 0 || to_rate == 0 || from_rate == to_rate || input.is_empty() {
        return input.to_vec();
    }
    let ratio = to_rate as f32 / from_rate as f32;
    let out_len = ((input.len() as f32) * ratio).round() as usize;
    if out_len == 0 {
        return Vec::new();
    }
    let last = input.len() - 1;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f32 / ratio;
        let i0 = (src.floor() as usize).min(last);
        let i1 = (i0 + 1).min(last);
        let frac = (src - i0 as f32).clamp(0.0, 1.0);
        out.push(input[i0] * (1.0 - frac) + input[i1] * frac);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_mono_averages_channels() {
        let stereo = [1.0f32, -1.0, 0.5, 0.5, 0.0, 0.0];
        assert_eq!(to_mono(&stereo, 2), vec![0.0, 0.5, 0.0]);
        assert_eq!(to_mono(&stereo, 1), stereo.to_vec());
        assert!(to_mono::<f32>(&[], 2).is_empty());
    }

    #[test]
    fn sample_format_to_f32_is_normalized() {
        assert_eq!(0i16.to_f32(), 0.0);
        assert_eq!(i16::MAX.to_f32() > 0.99, true);
        assert_eq!(i16::MIN.to_f32(), -1.0);
        assert_eq!(0u16.to_f32(), -1.0);
        assert_eq!(u16::MAX.to_f32() > 0.99, true);
    }

    #[test]
    fn resample_down_48k_to_16k() {
        let input = vec![0.0f32; 4800]; // 100ms @48k
        let out = resample_linear(&input, 48_000, 16_000);
        assert_eq!(out.len(), 1600); // 100ms @16k
    }

    #[test]
    fn resample_edge_cases_never_panic() {
        assert!(resample_linear(&[], 48_000, 16_000).is_empty());
        assert_eq!(resample_linear(&[0.5], 48_000, 16_000).len(), 0);
        assert_eq!(resample_linear(&[1.0, 2.0], 16_000, 16_000), vec![1.0, 2.0]);
        assert_eq!(resample_linear(&[1.0], 16_000, 0), vec![1.0]);
        let _ = resample_linear(&[1.0; 3], 16_000, 96_000); // 升采样越界保护
    }
}
