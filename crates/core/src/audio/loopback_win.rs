//! Windows 系统内录：WASAPI 回环采集（Loopback Capture）。
//!
//! **为什么不能用 cpal**：`cpal` 的 WASAPI 后端只在 capture endpoint（麦克风）上建流，
//! 而「系统声音」不是输入设备——它必须以 **render endpoint（输出设备）** 为对象，
//! 在 `IAudioClient::Initialize` 时带上 `AUDCLNT_STREAMFLAGS_LOOPBACK` 标志。
//! 这正是 Windows 上 `input_devices()` 永远看不到内录设备的原因。
//!
//! 实现要点：
//! - 共享模式必须使用设备 **mix format**（无法指定 16kHz），采集后自行混单 + 重采样。
//! - 采用轮询 `GetNextPacketSize`（事件驱动在部分系统/驱动上不触发，轮询更稳）。
//! - 未播放声音时 loopback 不产生数据包，这是正常行为（由上层 VAD / 超时处理）。
//! - 静音包（`AUDCLNT_BUFFERFLAGS_SILENT`）显式补零，保证时间轴连续。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use windows::core::GUID;
use windows::Win32::Media::Audio::{
    eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator,
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, DEVICE_STATE_ACTIVE, WAVEFORMATEX,
    WAVEFORMATEXTENSIBLE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
};

use super::capture::{resample_linear_pub as resample_linear, to_mono_f32, TARGET_SAMPLE_RATE};

/// `KSDATAFORMAT_SUBTYPE_IEEE_FLOAT` = {00000003-0000-0010-8000-00AA00389B71}
const SUBTYPE_IEEE_FLOAT: GUID = GUID::from_u128(0x0000_0003_0000_0010_8000_00aa_0038_9b71);

const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

const BUFFERFLAG_SILENT: u32 = 0x2;

/// 一个正在运行的回环采集会话（Drop 时停止并回收线程）。
pub struct LoopbackCapture {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    /// 实际设备信息（用于日志与诊断展示）。
    pub desc: String,
}

impl Drop for LoopbackCapture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// 打开第 `out_index` 个**输出设备**的回环采集。
///
/// `out_index` 与 `cpal::Host::output_devices()` 的枚举顺序一致（同为
/// `EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)`），因此前端由 cpal 枚举出的
/// 输出设备序号可直接用于这里。
pub fn start(
    out_index: usize,
    chunk_samples: usize,
    tx: Sender<Vec<f32>>,
) -> Result<LoopbackCapture, String> {
    let chunk = chunk_samples.clamp(256, 16384);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let (ready_tx, ready_rx) = channel::<Result<String, String>>();

    let handle = std::thread::Builder::new()
        .name(format!("wasapi-loopback-{out_index}"))
        .spawn(move || {
            if let Err(e) = run(out_index, chunk, tx, stop_thread, ready_tx.clone()) {
                // 若启动阶段已回报错误，这里只是线程退出；运行期错误记录日志
                let _ = ready_tx.send(Err(e.clone()));
                log::warn!("[audio] WASAPI 回环采集退出: {e}");
            }
        })
        .map_err(|e| format!("无法创建回环采集线程: {e}"))?;

    match ready_rx.recv_timeout(Duration::from_secs(6)) {
        Ok(Ok(desc)) => Ok(LoopbackCapture {
            stop,
            handle: Some(handle),
            desc,
        }),
        Ok(Err(e)) => {
            let _ = handle.join();
            Err(e)
        }
        Err(_) => {
            stop.store(true, Ordering::SeqCst);
            Err("WASAPI 回环采集启动超时（6 秒内未收到音频包，可能是该输出设备当前无活动会话）"
                .to_string())
        }
    }
}

/// 探测：能否为第 `out_index` 个输出设备建立回环流（不启动采集）。
pub fn probe(out_index: usize) -> Result<String, String> {
    unsafe {
        let _com = ComGuard::new();
        let client = open_client(out_index)?;
        let fmt = client.GetMixFormat().map_err(|e| format!("读取 mix format 失败: {e}"))?;
        let info = parse_format(fmt);
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK,
                0,
                0,
                fmt,
                None,
            )
            .map_err(|e| format!("回环初始化失败（该设备可能被独占）: {e}"))?;
        CoTaskMemFree(Some(fmt as *const core::ffi::c_void));
        Ok(format!(
            "WASAPI 回环 OK：{}ch/{}Hz/{}-bit {} → 内部混单/重采样为 16kHz 单声道 f32",
            info.channels, info.rate, info.bits, info.kind
        ))
    }
}

// ========================
// 内部实现
// ========================

struct ComGuard;

impl ComGuard {
    fn new() -> Self {
        unsafe {
            // S_FALSE（已初始化）与 RPC_E_CHANGED_MODE 都可继续使用
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        Self
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

struct FmtInfo {
    channels: usize,
    rate: u32,
    bits: u16,
    is_float: bool,
    kind: &'static str,
}

unsafe fn parse_format(p: *const WAVEFORMATEX) -> FmtInfo {
    let wf = &*p;
    let (is_float, kind) = match wf.wFormatTag {
        WAVE_FORMAT_IEEE_FLOAT => (true, "float"),
        WAVE_FORMAT_EXTENSIBLE => {
            // WAVEFORMATEXTENSIBLE 是 packed 结构，必须按未对齐方式读取 GUID
            let ext = p as *const WAVEFORMATEXTENSIBLE;
            let sub = std::ptr::read_unaligned(std::ptr::addr_of!((*ext).SubFormat));
            if sub == SUBTYPE_IEEE_FLOAT {
                (true, "float")
            } else {
                (false, "pcm")
            }
        }
        WAVE_FORMAT_PCM => (false, "pcm"),
        _ => (false, "unknown"),
    };
    FmtInfo {
        channels: wf.nChannels.max(1) as usize,
        rate: wf.nSamplesPerSec.max(1),
        bits: wf.wBitsPerSample,
        is_float,
        kind,
    }
}

unsafe fn open_client(out_index: usize) -> Result<IAudioClient, String> {
    let enumerator: IMMDeviceEnumerator =
        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
            .map_err(|e| format!("无法创建 WASAPI 设备枚举器: {e}"))?;
    let collection = enumerator
        .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
        .map_err(|e| format!("无法枚举输出设备（系统内录需要至少一个可用扬声器）: {e}"))?;
    let count = collection
        .GetCount()
        .map_err(|e| format!("无法获取输出设备数量: {e}"))?;
    if count == 0 {
        return Err("系统没有活动的输出设备（扬声器/耳机），无法进行系统内录".to_string());
    }
    if out_index as u32 >= count {
        return Err(format!(
            "输出设备序号越界：请求 {out_index}，实际仅 {count} 个输出设备"
        ));
    }
    let device = collection
        .Item(out_index as u32)
        .map_err(|e| format!("无法打开输出设备 #{out_index}: {e}"))?;
    device
        .Activate::<IAudioClient>(CLSCTX_ALL, None)
        .map_err(|e| format!("无法激活输出设备 #{out_index} 的音频客户端: {e}"))
}

fn run(
    out_index: usize,
    chunk: usize,
    tx: Sender<Vec<f32>>,
    stop: Arc<AtomicBool>,
    ready: Sender<Result<String, String>>,
) -> Result<(), String> {
    unsafe {
        let _com = ComGuard::new();

        let client = match open_client(out_index) {
            Ok(c) => c,
            Err(e) => {
                let _ = ready.send(Err(e.clone()));
                return Err(e);
            }
        };

        let fmt = match client.GetMixFormat() {
            Ok(f) => f,
            Err(e) => {
                let msg = format!("读取输出设备 mix format 失败: {e}");
                let _ = ready.send(Err(msg.clone()));
                return Err(msg);
            }
        };
        let info = parse_format(fmt);

        if let Err(e) = client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            0,
            0,
            fmt,
            None,
        ) {
            CoTaskMemFree(Some(fmt as *const core::ffi::c_void));
            let msg = format!(
                "回环初始化失败（设备可能被独占模式占用，或驱动不支持 loopback）: {e}"
            );
            let _ = ready.send(Err(msg.clone()));
            return Err(msg);
        }
        // Initialize 已复制格式内容，可释放
        CoTaskMemFree(Some(fmt as *const core::ffi::c_void));

        let capture: IAudioCaptureClient = match client.GetService() {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("获取 IAudioCaptureClient 失败: {e}");
                let _ = ready.send(Err(msg.clone()));
                return Err(msg);
            }
        };

        if let Err(e) = client.Start() {
            let msg = format!("启动回环采集失败: {e}");
            let _ = ready.send(Err(msg.clone()));
            return Err(msg);
        }

        let desc = format!(
            "WASAPI 回环 {}ch/{}Hz/{}-bit {} → 内部混单/重采样为 16kHz 单声道 f32（输出设备 #{out_index}）",
            info.channels, info.rate, info.bits, info.kind
        );
        let _ = ready.send(Ok(desc));

        let mut acc: Vec<f32> = Vec::with_capacity(chunk * 2);
        let mut first_packet = true;

        while !stop.load(Ordering::SeqCst) {
            let mut packet_frames = match capture.GetNextPacketSize() {
                Ok(n) => n,
                Err(_) => break,
            };

            if packet_frames == 0 {
                std::thread::sleep(Duration::from_millis(8));
                // 长时间完全无包（例如设备被拔掉）时不要死等，交给上层超时
                continue;
            }

            while packet_frames > 0 {
                let mut data: *mut u8 = std::ptr::null_mut();
                let mut frames = 0u32;
                let mut flags = 0u32;
                if capture
                    .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
                    .is_err()
                {
                    break;
                }

                let silent = flags & BUFFERFLAG_SILENT != 0;
                if frames > 0 {
                    let n = frames as usize * info.channels;
                    let mono = if silent || data.is_null() {
                        vec![0.0f32; frames as usize]
                    } else {
                        read_samples(data, n, info.bits, info.is_float, info.channels)
                    };
                    let mut resampled = resample_linear(&mono, info.rate, TARGET_SAMPLE_RATE);
                    acc.append(&mut resampled);
                }

                let _ = capture.ReleaseBuffer(frames);

                if first_packet && frames > 0 {
                    first_packet = false;
                }

                packet_frames = capture.GetNextPacketSize().unwrap_or(0);
            }

            // 按固定块大小吐出，避免前端收到过碎/过大的块
            while acc.len() >= chunk {
                let tail = acc.split_off(chunk);
                let block = std::mem::replace(&mut acc, tail);
                if tx.send(block).is_err() {
                    // 接收端已销毁
                    let _ = client.Stop();
                    return Ok(());
                }
            }
        }

        let _ = client.Stop();
        Ok(())
    }
}

/// 从原始缓冲区读取并转 f32（interleaved）。
unsafe fn read_samples(
    data: *mut u8,
    total: usize,
    bits: u16,
    is_float: bool,
    channels: usize,
) -> Vec<f32> {
    if is_float && bits == 32 {
        let src = std::slice::from_raw_parts(data as *const f32, total);
        return to_mono_f32(src, channels);
    }
    if !is_float {
        match bits {
            16 => {
                let src = std::slice::from_raw_parts(data as *const i16, total);
                let f: Vec<f32> = src.iter().map(|&v| v as f32 / 32_768.0).collect();
                return to_mono_f32(&f, channels);
            }
            32 => {
                // 可能是 32-bit PCM，也可能是 24-in-32；统一按 32 位归一
                let src = std::slice::from_raw_parts(data as *const i32, total);
                let f: Vec<f32> = src
                    .iter()
                    .map(|&v| v as f32 / 2_147_483_648.0)
                    .collect();
                return to_mono_f32(&f, channels);
            }
            24 => {
                // 24-bit packed little-endian
                let bytes = std::slice::from_raw_parts(data, total * 3);
                let f: Vec<f32> = bytes
                    .chunks_exact(3)
                    .map(|c| {
                        let v = (c[0] as i32) | ((c[1] as i32) << 8) | ((c[2] as i32) << 16);
                        let v = (v << 8) >> 8; // 符号扩展
                        v as f32 / 8_388_608.0
                    })
                    .collect();
                return to_mono_f32(&f, channels);
            }
            _ => {}
        }
    }
    // 未知格式：按静音处理，保证时间轴推进
    vec![0.0f32; total / channels.max(1)]
}
