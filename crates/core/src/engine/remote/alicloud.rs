//! 阿里云智能语音交互（NLS）实时语音识别云端 ASR。
//!
//! 参考 coretab `js/asr/asr-alicloud.js` 与阿里云官方文档：
//! - Token：`CreateToken`（RPC POP 签名，`nls-meta.cn-shanghai.aliyuncs.com`）
//! - 端点：`wss://nls-gateway-cn-shanghai.aliyuncs.com/ws/v1?token={token}`
//! - 握手：StartTranscription → TranscriptionStarted
//! - 识别：TranscriptionResultChanged / SentenceEnd，结束发 StopTranscription

use std::time::{SystemTime, UNIX_EPOCH};

use super::super::{AlicloudCreds, EngineConfig, EngineError, Segment, Source, TranscriptionEngine};
use super::{
    aliyun_rpc_signature, percent_encode, random_uuid, ParsedMessage, ProviderSpec, RawResult,
    RemoteSession,
};

/// 阿里云云端转写引擎。
pub struct AlicloudEngine {
    session: RemoteSession,
}

impl AlicloudEngine {
    pub fn connect(config: &EngineConfig, source: Source) -> Result<Self, EngineError> {
        let creds = config
            .remote
            .as_ref()
            .and_then(|r| r.alicloud.as_ref())
            .ok_or_else(|| {
                EngineError::Engine(
                    "缺少阿里云凭证（accessKeyId / accessKeySecret / appKey）".to_string(),
                )
            })?;

        if !creds.is_complete() {
            return Err(EngineError::Engine(
                "阿里云凭证不完整：需要 accessKeyId、accessKeySecret、appKey".to_string(),
            ));
        }

        // 先同步获取临时 Token
        let token = fetch_token(creds)?;
        let spec = build_spec(creds, &token, &config.model);
        let session = RemoteSession::spawn(spec, source)?;
        Ok(Self { session })
    }
}

impl TranscriptionEngine for AlicloudEngine {
    fn id(&self) -> &'static str {
        "alicloud"
    }

    fn feed(&mut self, samples: &[f32], _sample_rate: u32) -> Result<Vec<Segment>, EngineError> {
        self.session.feed(samples)
    }

    fn flush(&mut self) -> Result<Vec<Segment>, EngineError> {
        self.session.flush()
    }
}

/// 测试阿里云连接（Token 获取成功即视为凭证有效）。
pub(crate) fn test_connection(creds: &AlicloudCreds) -> Result<(), String> {
    if !creds.is_complete() {
        return Err("阿里云凭证不完整：需要 accessKeyId、accessKeySecret、appKey".to_string());
    }
    fetch_token(creds).map(|_| ()).map_err(|e| e.to_string())
}

/// 调用 CreateToken 获取临时 Token（POP 签名）。
fn fetch_token(creds: &AlicloudCreds) -> Result<String, EngineError> {
    let timestamp = utc_iso8601();
    let nonce = random_uuid().replace('-', "");

    let params: Vec<(&str, String)> = vec![
        ("AccessKeyId", creds.access_key_id.clone()),
        ("Action", "CreateToken".to_string()),
        ("Format", "JSON".to_string()),
        ("RegionId", "cn-shanghai".to_string()),
        ("SignatureMethod", "HMAC-SHA1".to_string()),
        ("SignatureNonce", nonce),
        ("SignatureVersion", "1.0".to_string()),
        ("Timestamp", timestamp),
        ("Version", "2019-07-17".to_string()),
    ];

    let sig_params: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let signature = aliyun_rpc_signature(&creds.access_key_secret, "GET", &sig_params);

    let mut query: Vec<String> = params
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect();
    query.push(format!("Signature={}", percent_encode(&signature)));

    let url = format!("https://nls-meta.cn-shanghai.aliyuncs.com/?{}", query.join("&"));

    let body = ureq::get(&url)
        .call()
        .map_err(|e| EngineError::Engine(format!("阿里云 Token 请求失败: {e}")))?
        .into_string()
        .map_err(|e| EngineError::Engine(format!("读取阿里云响应失败: {e}")))?;

    let v: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| EngineError::Engine(format!("解析阿里云响应失败: {e}")))?;

    if let Some(err) = v.get("Code").and_then(|c| c.as_str()) {
        let msg = v
            .get("Message")
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误");
        return Err(EngineError::Engine(format!("阿里云 Token 错误 [{err}]: {msg}")));
    }

    let token = v
        .get("Token")
        .and_then(|t| t.get("Id"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| {
            EngineError::Engine("阿里云 Token 响应缺少 Id（请检查 AccessKey 是否已开通语音服务）".to_string())
        })?
        .to_string();

    if token.is_empty() {
        return Err(EngineError::Engine("阿里云 Token 为空".to_string()));
    }

    Ok(token)
}

fn build_spec(creds: &AlicloudCreds, token: &str, model: &str) -> ProviderSpec {
    let ws_url = format!(
        "wss://nls-gateway-cn-shanghai.aliyuncs.com/ws/v1?token={}",
        percent_encode(token)
    );

    let task_id = random_uuid().replace('-', "");
    let message_id = random_uuid().replace('-', "");
    let model = if model.is_empty() {
        "domain-meeting-domain".to_string()
    } else {
        model.to_string()
    };

    let start_json = serde_json::json!({
        "header": {
            "message_id": message_id,
            "task_id": task_id,
            "namespace": "SpeechTranscriber",
            "name": "StartTranscription",
            "appkey": creds.app_key
        },
        "payload": {
            "format": "pcm",
            "sample_rate": 16000,
            "model": model,
            "enable_intermediate_result": true,
            "enable_punctuation_prediction": true,
            "enable_inverse_text_normalization": true
        }
    })
    .to_string();

    let stop_json = serde_json::json!({
        "header": {
            "message_id": random_uuid().replace('-', ""),
            "task_id": task_id,
            "namespace": "SpeechTranscriber",
            "name": "StopTranscription",
            "appkey": creds.app_key
        }
    })
    .to_string();

    ProviderSpec {
        ws_url,
        init_json: Some(start_json),
        require_handshake: true,
        end_json: Some(stop_json),
        flush_wait_ms: 900,
        parse: parse_alicloud,
    }
}

fn parse_alicloud(msg: &str) -> ParsedMessage {
    let mut out = ParsedMessage::default();
    let v: serde_json::Value = match serde_json::from_str(msg) {
        Ok(v) => v,
        Err(_) => return out,
    };

    let name = v
        .get("header")
        .and_then(|h| h.get("name"))
        .and_then(|n| n.as_str());
    let payload = v.get("payload").and_then(|p| p.as_object());

    match name {
        Some("TranscriptionStarted") => {
            out.ready = true;
        }
        Some("TranscriptionResultChanged") | Some("SentenceEnd") => {
            let text = payload
                .and_then(|p| p.get("result"))
                .and_then(|r| r.as_str())
                .unwrap_or("");
            let is_final = name == Some("SentenceEnd");
            if !text.is_empty() {
                out.results.push(RawResult {
                    text: text.to_string(),
                    is_final,
                    speaker: None,
                });
            }
        }
        Some("TaskFailed") => {
            let status = payload
                .and_then(|p| p.get("status_text"))
                .and_then(|s| s.as_str())
                .unwrap_or("未知错误");
            out.error = Some(format!("阿里云识别错误: {status}"));
        }
        _ => {}
    }

    out
}

/// 生成 UTC ISO8601 时间戳（阿里云 POP 签名要求，如 `2026-11-05T02:00:00Z`）。
fn utc_iso8601() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    let days = secs.div_euclid(86_400);
    let secs_of_day = secs.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = secs_of_day / 3600;
    let min = (secs_of_day % 3600) / 60;
    let sec = secs_of_day % 60;

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

/// Unix 天数 → 公历年月日（Howard Hinnant 算法）。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m as u32, d as u32)
}
