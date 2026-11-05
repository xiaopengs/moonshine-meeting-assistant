//! 云端 ASR 远程引擎框架。
//!
//! 统一通过后台线程 + tokio runtime 驱动 WebSocket 流式识别，
//! 主线程以同步 `feed`/`flush` 接口交互（与本地引擎接口一致）。
//!
//! 各服务商（火山/腾讯/阿里）通过 [`ProviderSpec`] 描述差异化行为：
//! 签名 URL、握手消息、结果解析、结束方式。协议细节参考 coretab 的
//! `js/asr/*.js` 实现。

mod alicloud;
mod sign;
mod tencent;
mod volcengine;

pub use alicloud::AlicloudEngine;
pub use tencent::TencentEngine;
pub use volcengine::VolcengineEngine;

use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc::{self as tmpsc, Receiver as TReceiver, Sender as TSender};
use tokio_tungstenite::tungstenite::Message;

use super::{EngineError, EngineKind, RemoteCredentials, Segment, Source};

pub(crate) use sign::{
    aliyun_rpc_signature, hmac_sha1_base64, hmac_sha256_hex, percent_encode, random_uuid,
};

/// 一条已解析的识别结果（尚未标注 source/speaker）。
#[derive(Debug, Clone)]
pub(crate) struct RawResult {
    pub text: String,
    pub is_final: bool,
    pub speaker: Option<String>,
}

/// 解析单条 WebSocket 文本消息的结果。
#[derive(Debug, Default)]
pub(crate) struct ParsedMessage {
    /// 该消息表示握手成功 / 服务就绪，可开始发送音频。
    pub ready: bool,
    /// 识别结果。
    pub results: Vec<RawResult>,
    /// 服务端返回的业务错误（非 None 则终止会话）。
    pub error: Option<String>,
}

/// 描述某个云服务商的行为（纯函数，无状态）。
pub(crate) struct ProviderSpec {
    /// 已签名的 WebSocket URL。
    pub ws_url: String,
    /// 连接建立后立即发送的初始化 JSON（火山/阿里需要）。
    pub init_json: Option<String>,
    /// 是否需等待服务端就绪后才发送音频（腾讯/阿里为 true）。
    pub require_handshake: bool,
    /// flush 时发送的结束 JSON（腾讯 `{"type":"end"}` / 阿里 StopTranscription）。
    pub end_json: Option<String>,
    /// flush 发送结束后等待的毫秒数（让服务端返回最后结果）。
    pub flush_wait_ms: u64,
    /// 消息解析函数。
    pub parse: fn(&str) -> ParsedMessage,
}

/// 后台线程收到的音频命令。
enum AudioCmd {
    /// 一段 PCM 16-bit 音频。
    Pcm(Vec<i16>),
    /// 结束一段话（发送 end 消息并收尾）。
    End,
}

/// 后台线程回传的事件。
enum RemoteEvent {
    Segment(Segment),
    Error(String),
}

/// 云端 ASR 会话句柄（主线程侧）。
pub(crate) struct RemoteSession {
    audio_tx: Option<TSender<AudioCmd>>,
    event_rx: Receiver<RemoteEvent>,
    handle: Option<JoinHandle<()>>,
    ended: bool,
}

impl RemoteSession {
    /// 启动后台 WebSocket 会话。
    pub fn spawn(spec: ProviderSpec, source: Source) -> Result<Self, EngineError> {
        let (audio_tx, audio_rx) = tmpsc::channel::<AudioCmd>(1024);
        let (event_tx, event_rx) = channel::<RemoteEvent>();

        let handle = std::thread::Builder::new()
            .name("remote-asr".to_string())
            .spawn(move || {
                let rt = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(rt) => rt,
                    Err(e) => {
                        let _ = event_tx.send(RemoteEvent::Error(format!("启动运行时失败: {e}")));
                        return;
                    }
                };
                rt.block_on(run_session(spec, source, audio_rx, event_tx));
            })
            .map_err(|e| EngineError::Spawn(e.to_string()))?;

        Ok(Self {
            audio_tx: Some(audio_tx),
            event_rx,
            handle: Some(handle),
            ended: false,
        })
    }

    /// 送入音频，返回已到达的识别结果。
    pub fn feed(&mut self, samples: &[f32]) -> Result<Vec<Segment>, EngineError> {
        if self.ended {
            return Ok(Vec::new());
        }
        let pcm = f32_to_i16(samples);
        let tx = self.audio_tx.as_ref().ok_or_else(|| {
            EngineError::Engine("云端 ASR 会话已关闭".to_string())
        })?;
        if tx.blocking_send(AudioCmd::Pcm(pcm)).is_err() {
            self.ended = true;
            return Err(EngineError::Engine(
                "云端 ASR 会话已断开（请检查网络或凭证）".to_string(),
            ));
        }
        self.drain()
    }

    /// 结束一句话，返回最终结果。
    pub fn flush(&mut self) -> Result<Vec<Segment>, EngineError> {
        if !self.ended {
            if let Some(tx) = &self.audio_tx {
                let _ = tx.blocking_send(AudioCmd::End);
            }
            self.ended = true;
        }
        // 等待后台线程回传最终结果并退出（最多 2 秒）
        let deadline = Instant::now() + Duration::from_millis(2000);
        loop {
            let segs = self.drain()?;
            if !segs.is_empty() {
                return Ok(segs);
            }
            if Instant::now() >= deadline || self.event_rx_is_closed() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        self.drain()
    }

    fn event_rx_is_closed(&self) -> bool {
        // try_recv 返回 Disconnected 表示后台已退出、通道关闭
        matches!(self.event_rx.try_recv(), Err(TryRecvError::Disconnected))
    }

    /// 非阻塞取回所有事件。
    fn drain(&mut self) -> Result<Vec<Segment>, EngineError> {
        let mut segs = Vec::new();
        let mut first_err: Option<String> = None;
        loop {
            match self.event_rx.try_recv() {
                Ok(RemoteEvent::Segment(s)) => segs.push(s),
                Ok(RemoteEvent::Error(e)) => {
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        if let Some(e) = first_err {
            return Err(EngineError::Engine(e));
        }
        Ok(segs)
    }
}

impl Drop for RemoteSession {
    fn drop(&mut self) {
        if !self.ended {
            if let Some(tx) = &self.audio_tx {
                let _ = tx.blocking_send(AudioCmd::End);
            }
        }
        // 关闭发送端，让后台线程 recv 返回 None 并退出
        self.audio_tx = None;
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// 通用 WebSocket 会话循环。
async fn run_session(
    spec: ProviderSpec,
    source: Source,
    mut audio_rx: TReceiver<AudioCmd>,
    event_tx: Sender<RemoteEvent>,
) {
    let ws_stream = match tokio_tungstenite::connect_async(spec.ws_url.as_str()).await {
        Ok((ws, _)) => ws,
        Err(e) => {
            let _ = event_tx.send(RemoteEvent::Error(format!("WebSocket 连接失败: {e}")));
            return;
        }
    };

    let (mut write, mut read) = ws_stream.split();
    let mut audio_ok = !spec.require_handshake;

    // 发送初始化消息
    if let Some(init) = &spec.init_json {
        if write.send(Message::Text(init.clone())).await.is_err() {
            let _ = event_tx.send(RemoteEvent::Error("发送初始化消息失败".to_string()));
            return;
        }
    }

    let mut flush_deadline: Option<tokio::time::Instant> = None;

    loop {
        tokio::select! {
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Text(t))) => {
                        let parsed = (spec.parse)(&t);
                        if let Some(e) = parsed.error {
                            let _ = event_tx.send(RemoteEvent::Error(e));
                            break;
                        }
                        if parsed.ready {
                            audio_ok = true;
                        }
                        for r in parsed.results {
                            if r.text.trim().is_empty() { continue; }
                            let seg = Segment {
                                source,
                                speaker: r.speaker.clone().or_else(|| Some(source.default_speaker().to_string())),
                                text: r.text,
                                start: 0.0,
                                end: 0.0,
                                is_final: r.is_final,
                            };
                            let _ = event_tx.send(RemoteEvent::Segment(seg));
                        }
                    }
                    Some(Ok(Message::Binary(_))) => {}
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => break,
                    _ => {}
                }
            }
            audio = audio_rx.recv() => {
                match audio {
                    Some(AudioCmd::Pcm(pcm)) => {
                        if audio_ok {
                            let bytes = i16_to_le_bytes(&pcm);
                            if write.send(Message::Binary(bytes)).await.is_err() {
                                break;
                            }
                        }
                    }
                    Some(AudioCmd::End) => {
                        // 发送结束消息并等待服务端回传最终结果
                        if let Some(end) = &spec.end_json {
                            let _ = write.send(Message::Text(end.clone())).await;
                        }
                        flush_deadline = Some(tokio::time::Instant::now()
                            + Duration::from_millis(spec.flush_wait_ms.max(300)));
                    }
                    None => break,
                }
            }
        }

        // flush 计时结束（已发送 end 消息并等待足够时长）后退出
        if let Some(deadline) = flush_deadline {
            if tokio::time::Instant::now() >= deadline {
                break;
            }
        }
    }

    let _ = write.close().await;
}

/// f32 样本 → PCM 16-bit（对齐 coretab `floatTo16BitPCM` 的限幅规则）。
fn f32_to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|&s| {
            let s = s.clamp(-1.0, 1.0);
            if s < 0.0 {
                (s * 32768.0) as i16
            } else {
                (s * 32767.0) as i16
            }
        })
        .collect()
}

/// PCM 16-bit → little-endian 字节流。
fn i16_to_le_bytes(samples: &[i16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// 测试云端 ASR 连接（验证凭证是否有效）。
pub fn test_connection(
    kind: EngineKind,
    creds: &RemoteCredentials,
    model: &str,
) -> Result<(), String> {
    match kind {
        EngineKind::Volcengine => {
            let c = creds
                .volcengine
                .as_ref()
                .ok_or("缺少火山引擎凭证（accessKeyId/secretKey/appId）")?;
            volcengine::test_connection(c, model)
        }
        EngineKind::Tencent => {
            let c = creds
                .tencent
                .as_ref()
                .ok_or("缺少腾讯云凭证（appId/secretId/secretKey）")?;
            tencent::test_connection(c, model)
        }
        EngineKind::Alicloud => {
            let c = creds
                .alicloud
                .as_ref()
                .ok_or("缺少阿里云凭证（accessKeyId/accessKeySecret/appKey）")?;
            alicloud::test_connection(c)
        }
        _ => Err("该引擎为本地引擎，无需测试云端连接".to_string()),
    }
}

/// 建立 WebSocket 握手以验证签名与凭证（火山/腾讯共用）。
pub(crate) fn test_ws_handshake(spec: &ProviderSpec) -> Result<(), String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;

    rt.block_on(async {
        let (ws, _) = tokio_tungstenite::connect_async(spec.ws_url.as_str())
            .await
            .map_err(|e| e.to_string())?;
        let (mut write, mut read) = ws.split();

        if let Some(init) = &spec.init_json {
            write
                .send(Message::Text(init.clone()))
                .await
                .map_err(|e| e.to_string())?;
        }

        let deadline = tokio::time::Instant::now() + Duration::from_millis(8000);
        loop {
            tokio::select! {
                msg = read.next() => {
                    match msg {
                        Some(Ok(Message::Text(t))) => {
                            let parsed = (spec.parse)(&t);
                            if let Some(e) = parsed.error {
                                return Err(e);
                            }
                            if parsed.ready {
                                return Ok(());
                            }
                        }
                        Some(Ok(Message::Close(_))) | Some(Err(_)) | None => {
                            return Err("连接被关闭，凭证可能无效".to_string());
                        }
                        _ => {}
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    return Err("连接超时，请检查网络或凭证".to_string());
                }
            }
        }
    })
}
