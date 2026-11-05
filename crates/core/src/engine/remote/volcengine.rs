//! 火山引擎（豆包流式语音识别 2.0）云端 ASR。
//!
//! 参考 coretab `js/asr/asr-volcengine.js` 与火山引擎官方文档：
//! - 端点：`wss://openspeech.bytedance.com/api/v3/sauc/bigmodel`
//! - 鉴权：HMAC-SHA256 签名（`accessKeyId\ntimestamp\nsessionId\n`）
//! - 握手：连接后发送 init JSON，随后持续发送 16k/16bit 单声道 PCM

use std::time::{SystemTime, UNIX_EPOCH};

use super::super::{
    EngineConfig, EngineError, Segment, Source, TranscriptionEngine, VolcengineCreds,
};
use super::{hmac_sha256_hex, random_uuid, ParsedMessage, ProviderSpec, RawResult, RemoteSession};

/// 火山引擎云端转写引擎。
pub struct VolcengineEngine {
    session: RemoteSession,
}

impl VolcengineEngine {
    pub fn connect(config: &EngineConfig, source: Source) -> Result<Self, EngineError> {
        let creds = config
            .remote
            .as_ref()
            .and_then(|r| r.volcengine.as_ref())
            .ok_or_else(|| {
                EngineError::Engine("缺少火山引擎凭证（accessKeyId / secretKey / appId）".to_string())
            })?;

        if !creds.is_complete() {
            return Err(EngineError::Engine(
                "火山引擎凭证不完整：需要 accessKeyId、secretKey、appId".to_string(),
            ));
        }

        let spec = build_spec(creds, &config.model);
        let session = RemoteSession::spawn(spec, source)?;
        Ok(Self { session })
    }
}

impl TranscriptionEngine for VolcengineEngine {
    fn id(&self) -> &'static str {
        "volcengine"
    }

    fn feed(&mut self, samples: &[f32], _sample_rate: u32) -> Result<Vec<Segment>, EngineError> {
        self.session.feed(samples)
    }

    fn flush(&mut self) -> Result<Vec<Segment>, EngineError> {
        self.session.flush()
    }
}

/// 测试火山引擎连接（握手验证签名）。
pub(crate) fn test_connection(creds: &VolcengineCreds, model: &str) -> Result<(), String> {
    if !creds.is_complete() {
        return Err("火山引擎凭证不完整：需要 accessKeyId、secretKey、appId".to_string());
    }
    let spec = build_spec(creds, model);
    super::test_ws_handshake(&spec)
}

fn build_spec(creds: &VolcengineCreds, model: &str) -> ProviderSpec {    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let session_id = random_uuid();
    let message = format!("{}\n{}\n{}\n", creds.access_key_id, timestamp, session_id);
    let signature = hmac_sha256_hex(&creds.secret_key, &message);

    let ws_url = format!(
        "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel?access_key_id={}&app_id={}&timestamp={}&signature={}",
        creds.access_key_id, creds.app_id, timestamp, signature
    );

    let model = if model.is_empty() { "bigmodel" } else { model };
    let init_json = serde_json::json!({
        "app": {
            "appid": creds.app_id,
            "clusterid": "volcengine_streaming_common",
            "token": "access_token"
        },
        "user": { "uid": "moonshine_meeting" },
        "audio": { "format": "pcm", "rate": 16000, "bits": 16, "channel": 1, "codec": "raw" },
        "request": {
            "model": model,
            "result_type": "single",
            "show_utterances": true,
            "silence_wait": 3000
        }
    })
    .to_string();

    ProviderSpec {
        ws_url,
        init_json: Some(init_json),
        require_handshake: false,
        end_json: None,
        flush_wait_ms: 800,
        parse: parse_volcengine,
    }
}

fn parse_volcengine(msg: &str) -> ParsedMessage {
    let mut out = ParsedMessage::default();
    let v: serde_json::Value = match serde_json::from_str(msg) {
        Ok(v) => v,
        Err(_) => return out,
    };

    if let Some(code) = v.get("code").and_then(|c| c.as_i64()) {
        if code != 1000 {
            let message = v
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("未知错误");
            out.error = Some(format!("火山引擎错误 [{code}]: {message}"));
            return out;
        }
        // code=1000 表示鉴权通过、服务就绪
        out.ready = true;
    }

    if let Some(result) = v.get("result") {
        let text = result
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        let is_final = result
            .get("is_final")
            .and_then(|f| f.as_bool())
            .unwrap_or(false);
        if !text.is_empty() {
            out.results.push(RawResult {
                text,
                is_final,
                speaker: None,
            });
        }
    }
    out
}
