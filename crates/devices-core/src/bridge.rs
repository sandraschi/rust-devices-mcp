//! Fleet cross-connect client: this Rust app talks to its Python siblings
//! over their localhost HTTP surfaces. NO new bridge server — the fleet
//! already runs the servers (aiwatcher, speech-mcp, robofang); this module
//! is the Rust-native caller with typed errors and one shared client.
//!
//! Answer to "do we need a new Rust crossconnect bridge": no. A bridge
//! *server* would duplicate running infrastructure; a thin *client* module
//! is the whole need. If a future repo needs Rust-to-Rust service calls,
//! revisit — until then, HTTP POSTs carry every story:
//! house events → aiwatcher feed, spoken announcements → speech-mcp,
//! anything else → generic webhook (robofang, ntfy, user scripts).
use crate::creds::{classify_vendor_error, with_timeout_secs};
use crate::error::DeviceError;
use std::time::Duration;

pub const DEFAULT_AIWATCHER: &str = "http://127.0.0.1:10946";
pub const DEFAULT_SPEECH: &str = "http://127.0.0.1:10909";

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn env_non_empty(name: &str) -> Option<String> {
    clean(std::env::var(name).ok())
}

fn http_client() -> Result<reqwest::Client, DeviceError> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| DeviceError::Protocol(format!("bridge http client: {e}")))
}

/// Resolve a sibling base URL: explicit arg → FLEET env → localhost default.
pub fn base_url(service: &str, explicit: Option<String>) -> String {
    let (env_name, default) = match service {
        "aiwatcher" => ("AIWATCHER_BASE_URL", DEFAULT_AIWATCHER),
        "speech" => ("SPEECH_BASE_URL", DEFAULT_SPEECH),
        _ => ("FLEET_BASE_URL", "http://127.0.0.1:1"),
    };
    clean(explicit)
        .or_else(|| env_non_empty(env_name))
        .unwrap_or_else(|| default.to_string())
        .trim_end_matches('/')
        .to_string()
}

/// Push a structured event into aiwatcher's Fleet Events feed.
/// POST {base}/api/fleet/ingest {title, summary?, source?, url?, urgency_hint?}
pub async fn aiwatcher_event(
    base: Option<String>,
    title: &str,
    summary: Option<String>,
    urgency: Option<f64>,
) -> Result<serde_json::Value, DeviceError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(DeviceError::InvalidInput(
            "event title must not be empty".to_string(),
        ));
    }
    let base = base_url("aiwatcher", base);
    let http = http_client()?;
    let mut body = serde_json::json!({
        "title": title,
        "summary": summary.unwrap_or_default(),
        "source": "rust-devices-mcp",
    });
    if let Some(urgency) = urgency.filter(|u| u.is_finite()) {
        body["urgency_hint"] = urgency.into();
    }
    let resp = with_timeout_secs(
        "bridge.aiwatcher",
        12,
        http.post(format!("{base}/api/fleet/ingest"))
            .json(&body)
            .send(),
    )
    .await?
    .map_err(|e| classify_vendor_error(format!("bridge.aiwatcher: {e}")))?;
    decode_json("bridge.aiwatcher", resp).await
}

/// Speak text on the server speaker via speech-mcp.
/// POST {base}/api/v1/tts {text} (provider defaults server-side).
pub async fn speak(
    base: Option<String>,
    text: &str,
    provider: Option<String>,
) -> Result<serde_json::Value, DeviceError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(DeviceError::InvalidInput(
            "speech text must not be empty".to_string(),
        ));
    }
    if text.chars().count() > 1000 {
        return Err(DeviceError::InvalidInput(
            "speech text over 1000 chars — split it".to_string(),
        ));
    }
    let base = base_url("speech", base);
    let http = http_client()?;
    let mut body = serde_json::json!({ "text": text });
    if let Some(provider) = clean(provider) {
        body["provider"] = provider.into();
    }
    let resp = with_timeout_secs(
        "bridge.speak",
        30,
        http.post(format!("{base}/api/v1/tts")).json(&body).send(),
    )
    .await?
    .map_err(|e| classify_vendor_error(format!("bridge.speak: {e}")))?;
    decode_json("bridge.speak", resp).await
}

/// Generic webhook POST {title, summary, source} to any URL (robofang,
/// ntfy, user scripts). Only http(s) targets — no file/odd schemes.
pub async fn webhook(
    url: &str,
    title: &str,
    summary: Option<String>,
) -> Result<serde_json::Value, DeviceError> {
    let url = url.trim();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(DeviceError::InvalidInput(format!(
            "webhook url must be http(s): '{url}'"
        )));
    }
    if title.trim().is_empty() {
        return Err(DeviceError::InvalidInput(
            "webhook title must not be empty".to_string(),
        ));
    }
    let http = http_client()?;
    let resp = with_timeout_secs(
        "bridge.webhook",
        12,
        http.post(url)
            .json(&serde_json::json!({
                "title": title.trim(),
                "summary": summary.unwrap_or_default(),
                "source": "rust-devices-mcp",
            }))
            .send(),
    )
    .await?
    .map_err(|e| classify_vendor_error(format!("bridge.webhook: {e}")))?;
    decode_json("bridge.webhook", resp).await
}

/// Liveness probe: GET {base}/api/health.
pub async fn health(base: Option<String>, service: &str) -> Result<bool, DeviceError> {
    let base = base_url(service, base);
    let http = http_client()?;
    let resp = with_timeout_secs(
        "bridge.health",
        8,
        http.get(format!("{base}/api/health")).send(),
    )
    .await?
    .map_err(|e| classify_vendor_error(format!("bridge.health: {e}")))?;
    Ok(resp.status().is_success())
}

async fn decode_json(op: &str, resp: reqwest::Response) -> Result<serde_json::Value, DeviceError> {
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| DeviceError::Protocol(format!("{op} body: {e}")))?;
    if !status.is_success() {
        return Err(DeviceError::Protocol(format!("{op} HTTP {status}: {text}")));
    }
    if text.trim().is_empty() {
        return Ok(serde_json::json!({"ok": true}));
    }
    serde_json::from_str(&text).map_err(|e| DeviceError::Protocol(format!("{op} decode: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_prefers_explicit_then_default() {
        assert_eq!(
            base_url("aiwatcher", Some("http://x:1/".to_string())),
            "http://x:1"
        );
        assert_eq!(base_url("aiwatcher", None), DEFAULT_AIWATCHER);
    }

    #[tokio::test]
    async fn empty_inputs_rejected_before_any_network() {
        // Validation precedes all I/O: no request leaves the machine.
        assert!(matches!(
            aiwatcher_event(None, "   ", None, None).await,
            Err(DeviceError::InvalidInput(_))
        ));
        assert!(matches!(
            speak(None, "", None).await,
            Err(DeviceError::InvalidInput(_))
        ));
        assert!(matches!(
            speak(None, &"x".repeat(1001), None).await,
            Err(DeviceError::InvalidInput(_))
        ));
        assert!(matches!(
            webhook("file:///etc/passwd", "x", None).await,
            Err(DeviceError::InvalidInput(_))
        ));
        assert!(matches!(
            webhook("https://example.invalid/hook", "", None).await,
            Err(DeviceError::InvalidInput(_))
        ));
    }
}
