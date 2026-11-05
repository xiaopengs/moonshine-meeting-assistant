//! 腾讯云实时语音识别 V2（WebSocket）云端 ASR。
//!
//! 参考 coretab `js/asr/asr-tencent.js` 与腾讯云官方文档：
//! - 端点：`wss://asr.cloud.tencent.com/asr/v2/{appid}`
//! - 鉴权：HMAC-SHA1 签名（`asr.cloud.tencent.com/asr/v2/{appid}?{字典序参数}`）
//! - 握手：首条 `code=0`；识别发送二进制 PCM，结束发 `{"type":"end"}`

use std::time::{SystemTime, UNIX_EPOCH};

use super::super::{EngineConfig, EngineError, Segment, Source, TencentCreds, TranscriptionEngine};
use super::{
    hmac_sha1_base64, percent_encode, random_uuid, ParsedMessage, ProviderSpec, RawResult,
    RemoteSession,
};

/// 旧版引擎名 → V2 大模型引擎名。
fn legacy_model(model: &str) -> String {
    match model {
        "16k_zh_large" => "16k_zh_en_2.0".to_string(),
        "16k_zh_large_speaker" => "16k_zh_en_speaker_2.0".to_string(),
        "" => "16k_zh_en_2.0".to_string(),
        m => m.to_string(),
    }
}

/// 腾讯云云端转写引擎。
pub struct TencentEngine {
    session: RemoteSession,
}

impl TencentEngine {
    pub fn connect(config: &EngineConfig, source: Source) -> Result<Self, EngineError> {
        let creds = config
            .remote
            .as_ref()
            .and_then(|r| r.tencent.as_ref())
            .ok_or_else(|| {
                EngineError::Engine("缺少腾讯云凭证（appId / secretId / secretKey）".to_string())
            })?;

        if !creds.is_complete() {
            return Err(EngineError::Engine(
                "腾讯云凭证不完整：需要 appId、secretId、secretKey".to_string(),
            ));
        }

        let spec = build_spec(creds, &config.model);
        let session = RemoteSession::spawn(spec, source)?;
        Ok(Self { session })
    }
}

impl TranscriptionEngine for TencentEngine {
    fn id(&self) -> &'static str {
        "tencent"
    }

    fn feed(&mut self, samples: &[f32], _sample_rate: u32) -> Result<Vec<Segment>, EngineError> {
        self.session.feed(samples)
    }

    fn flush(&mut self) -> Result<Vec<Segment>, EngineError> {
        self.session.flush()
    }
}

/// 测试腾讯云连接（握手验证签名）。
pub(crate) fn test_connection(creds: &TencentCreds, model: &str) -> Result<(), String> {
    if !creds.is_complete() {
        return Err("腾讯云凭证不完整：需要 appId、secretId、secretKey".to_string());
    }
    let spec = build_spec(creds, model);
    super::test_ws_handshake(&spec)
}

fn build_spec(creds: &TencentCreds, model: &str) -> ProviderSpec {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let nonce = format!("{:010}", now % 10_000_000_000);
    let voice_id = random_uuid();
    let model = legacy_model(model);

    let params: Vec<(&str, String)> = vec![
        ("secretid", creds.secret_id.clone()),
        ("timestamp", now.to_string()),
        ("expired", (now + 86400).to_string()),
        ("nonce", nonce),
        ("engine_model_type", model),
        ("voice_id", voice_id),
        ("voice_format", "1".to_string()),
        ("needvad", "1".to_string()),
        ("filter_dirty", "0".to_string()),
        ("filter_modal", "0".to_string()),
        ("convert_num_mode", "1".to_string()),
    ];

    let mut sorted = params;
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    let query = sorted
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");

    let source = format!("asr.cloud.tencent.com/asr/v2/{}?{query}", creds.app_id);
    let signature = hmac_sha1_base64(&creds.secret_key, &source);
    let ws_url = format!(
        "wss://asr.cloud.tencent.com/asr/v2/{}?{query}&signature={}",
        creds.app_id,
        percent_encode(&signature)
    );

    ProviderSpec {
        ws_url,
        init_json: None,
        require_handshake: true,
        end_json: Some(r#"{"type":"end"}"#.to_string()),
        flush_wait_ms: 900,
        parse: parse_tencent,
    }
}

fn parse_tencent(msg: &str) -> ParsedMessage {
    let mut out = ParsedMessage::default();
    let v: serde_json::Value = match serde_json::from_str(msg) {
        Ok(v) => v,
        Err(_) => return out,
    };

    if let Some(code) = v.get("code").and_then(|c| c.as_i64()) {
        if code != 0 {
            let message = v
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("未知错误");
            out.error = Some(format!("腾讯云错误 [{code}]: {message}"));
            return out;
        }
        // code=0 表示握手成功
        out.ready = true;
    }

    let stream_final = v.get("final").and_then(|f| f.as_i64()) == Some(1);

    // 结构一：result（通用 / 大模型引擎）
    if let Some(r) = v.get("result").and_then(|r| r.as_object()) {
        let text = r
            .get("voice_text_str")
            .or_else(|| r.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("");
        let slice_type = r.get("slice_type").and_then(|s| s.as_i64()).unwrap_or(0);
        let speaker = r.get("speaker_id").and_then(|s| s.as_i64());
        if !text.is_empty() {
            out.results.push(RawResult {
                text: text.to_string(),
                is_final: slice_type == 2 || stream_final,
                speaker: speaker.filter(|&s| s >= 0).map(|s| s.to_string()),
            });
        }
        return out;
    }

    // 结构二：sentences（说话人分离引擎）
    if let Some(s) = v.get("sentences").and_then(|s| s.as_object()) {
        let items: Vec<&serde_json::Value> = if let Some(l) = s.get("sentence_list").and_then(|l| l.as_array()) {
            l.iter().collect()
        } else if let Some(x) = s.get("sentence") {
            vec![x]
        } else {
            return out;
        };
        for item in items {
            let text = item
                .get("sentence")
                .or_else(|| item.get("voice_text_str"))
                .or_else(|| item.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            let sentence_type = item
                .get("sentence_type")
                .and_then(|t| t.as_i64())
                .unwrap_or(0);
            let speaker = item.get("speaker_id").and_then(|s| s.as_i64());
            if !text.is_empty() {
                out.results.push(RawResult {
                    text: text.to_string(),
                    is_final: sentence_type == 1 || stream_final,
                    speaker: speaker.filter(|&s| s >= 0).map(|s| s.to_string()),
                });
            }
        }
    }

    out
}
