//! 单路音频采集封装：统一输出 16kHz 单声道 f32 块。

use std::sync::mpsc::{channel, Receiver};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use thiserror::Error;

/// 输出采样率（moonshine / faster-whisper 约定）。
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("音频宿主错误: {0}")]
    Host(String),
    #[error("未找到设备 index={0}")]
    DeviceNotFound(usize),
    #[error("无法打开输入流: {0}")]
    Stream(String),
}

/// 一个音频输入流的采集封装。
///
/// 内部将设备数据转换为 16kHz 单声道，通过 [`AudioCapture::try_recv`] 非阻塞取块。
pub struct AudioCapture {
    _stream: cpal::Stream,
    rx: Receiver<Vec<f32>>,
}

impl AudioCapture {
    /// 打开指定索引的输入设备。
    ///
    /// 优先以 16kHz 单声道打开；若设备不支持，则回退到设备默认配置并在回调内完成
    /// 声道混单与线性重采样，对外始终输出 16kHz 单声道。
    pub fn open(index: usize, chunk_samples: usize) -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let device = host
            .input_devices()
            .map_err(|e| AudioError::Host(e.to_string()))?
            .nth(index)
            .ok_or(AudioError::DeviceNotFound(index))?;

        let (tx, rx) = channel::<Vec<f32>>();

        let preferred = cpal::StreamConfig {
            channels: 1,
            sample_rate: cpal::SampleRate(TARGET_SAMPLE_RATE),
            buffer_size: cpal::BufferSize::Fixed(chunk_samples as u32),
        };

        let stream = match device.build_input_stream(
            &preferred,
            {
                let tx = tx.clone();
                move |data: &[f32], _| {
                    let _ = tx.send(data.to_vec());
                }
            },
            err_fn,
            None,
        ) {
            Ok(s) => s,
            Err(_) => {
                // 回退：设备默认配置 + 混单 + 重采样
                let supported = device
                    .default_input_config()
                    .map_err(|e| AudioError::Stream(e.to_string()))?;
                let channels = supported.channels() as usize;
                let src_rate = supported.sample_rate().0;
                let fallback = cpal::StreamConfig {
                    channels: supported.channels(),
                    sample_rate: supported.sample_rate(),
                    buffer_size: cpal::BufferSize::Fixed(chunk_samples as u32),
                };
                device
                    .build_input_stream(
                        &fallback,
                        move |data: &[f32], _| {
                            let mono = to_mono(data, channels);
                            let resampled = resample_linear(&mono, src_rate, TARGET_SAMPLE_RATE);
                            let _ = tx.send(resampled);
                        },
                        err_fn,
                        None,
                    )
                    .map_err(|e| AudioError::Stream(e.to_string()))?
            }
        };

        stream.play().map_err(|e| AudioError::Stream(e.to_string()))?;
        Ok(Self {
            _stream: stream,
            rx,
        })
    }

    /// 非阻塞取回一个音频块。
    pub fn try_recv(&self) -> Option<Vec<f32>> {
        self.rx.try_recv().ok()
    }
}

fn err_fn(err: cpal::StreamError) {
    log::warn!("[audio] 采集流错误: {err}");
}

/// interleaved 多声道 → 单声道（逐样本平均）。
fn to_mono(data: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return data.to_vec();
    }
    (0..data.len() / channels)
        .map(|i| {
            let sum: f32 = (0..channels).map(|c| data[i * channels + c]).sum();
            sum / channels as f32
        })
        .collect()
}

/// 线性插值重采样。
fn resample_linear(input: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate || input.is_empty() {
        return input.to_vec();
    }
    let ratio = to_rate as f32 / from_rate as f32;
    let out_len = ((input.len() as f32) * ratio).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f32 / ratio;
        let i0 = src.floor() as usize;
        let i1 = (i0 + 1).min(input.len() - 1);
        let frac = src - i0 as f32;
        out.push(input[i0] * (1.0 - frac) + input[i1] * frac);
    }
    out
}
