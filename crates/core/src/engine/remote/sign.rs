//! 云端 ASR 鉴权签名工具。
//!
//! - 火山引擎：HMAC-SHA256（hex）
//! - 腾讯云：HMAC-SHA1（Base64）
//! - 阿里云：RPC POP 签名（HMAC-SHA1 + RFC3986 percent-encode）

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::Sha256;

type HmacSha1 = Hmac<Sha1>;
type HmacSha256 = Hmac<Sha256>;

/// HMAC-SHA256 输出小写 hex（火山引擎签名）。
pub fn hmac_sha256_hex(key: &str, msg: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(key.as_bytes()).expect("HMAC 接受任意长度 key");
    mac.update(msg.as_bytes());
    let out = mac.finalize().into_bytes();
    out.iter().map(|b| format!("{b:02x}")).collect()
}

/// HMAC-SHA1 输出 Base64（腾讯云签名）。
pub fn hmac_sha1_base64(key: &str, msg: &str) -> String {
    let mut mac = HmacSha1::new_from_slice(key.as_bytes()).expect("HMAC 接受任意长度 key");
    mac.update(msg.as_bytes());
    B64.encode(mac.finalize().into_bytes())
}

/// RFC3986 percent-encode（保留 `A-Za-z0-9-_.~`）。
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// 阿里云 RPC POP 签名（SignatureVersion 1.0）。
///
/// `params` 为除 `Signature` 外的全部请求参数（key/value 均为未编码原文）。
pub fn aliyun_rpc_signature(
    access_key_secret: &str,
    http_method: &str,
    params: &[(&str, &str)],
) -> String {
    let mut sorted: Vec<(&str, &str)> = params.to_vec();
    sorted.sort_by(|a, b| a.0.cmp(b.0));

    let canonical: Vec<String> = sorted
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect();
    let canonical_query = canonical.join("&");

    let string_to_sign = format!(
        "{}&{}&{}",
        http_method,
        percent_encode("/"),
        percent_encode(&canonical_query)
    );

    // 阿里云 POP 签名 key 需在 AccessKeySecret 后追加 "&"
    let key = format!("{access_key_secret}&");
    hmac_sha1_base64(&key, &string_to_sign)
}

/// 生成随机 UUID v4 风格字符串（用作 session_id / voice_id / task_id）。
pub fn random_uuid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mut x = nanos ^ (std::process::id() as u128) << 32;
    let hex = |x: &mut u128, n: usize| {
        let mut s = String::new();
        for _ in 0..n {
            *x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let b = (*x >> 33) as u8 & 0xf;
            s.push_str(&format!("{b:x}"));
        }
        s
    };
    format!(
        "{}-{}-4{}-{}-{}",
        hex(&mut x, 8),
        hex(&mut x, 4),
        hex(&mut x, 3),
        hex(&mut x, 3),
        hex(&mut x, 12)
    )
}
