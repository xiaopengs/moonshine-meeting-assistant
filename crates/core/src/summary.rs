//! 会议摘要：OpenAI 兼容端点（本地 Ollama / LM Studio / 可选云端）。

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 摘要配置（OpenAI 兼容 Chat Completions 协议）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryConfig {
    /// 完整 chat/completions 端点，例如：
    /// `http://localhost:11434/v1/chat/completions`（Ollama）
    /// 或 `https://api.openai.com/v1/chat/completions`
    pub endpoint: String,
    pub model: String,
    #[serde(default)]
    pub api_key: Option<String>,
}

impl Default for SummaryConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:11434/v1/chat/completions".to_string(),
            model: "qwen2.5:7b".to_string(),
            api_key: None,
        }
    }
}

#[derive(Debug, Error)]
pub enum SummaryError {
    #[error("HTTP 请求失败: {0}")]
    Http(String),
    #[error("响应解析失败: {0}")]
    Parse(String),
}

const SYSTEM_PROMPT: &str = "你是专业的会议纪要助手。请根据会议转写内容，生成简洁、结构化的中文会议摘要，必须包含：1) 核心议题；2) 关键结论；3) 待办事项（若有）。不要编造转写中不存在的内容。";

/// 根据转写文本生成会议摘要。
pub fn summarize(config: &SummaryConfig, transcript: &str) -> Result<String, SummaryError> {
    let body = serde_json::json!({
        "model": config.model,
        "messages": [
            { "role": "system", "content": SYSTEM_PROMPT },
            { "role": "user", "content": transcript },
        ],
        "temperature": 0.3,
        "stream": false,
    });

    let mut req = ureq::post(&config.endpoint);
    if let Some(key) = &config.api_key {
        if !key.is_empty() {
            req = req.set("Authorization", &format!("Bearer {key}"));
        }
    }

    let resp: serde_json::Value = req
        .send_json(body)
        .map_err(|e| SummaryError::Http(e.to_string()))?
        .into_json()
        .map_err(|e| SummaryError::Parse(e.to_string()))?;

    resp["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| SummaryError::Parse("响应缺少 choices[0].message.content".to_string()))
}
