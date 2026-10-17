//! Philips Hue lighting via the bridge CLIP v1 REST API.
//!
//! Hand-rolled on `reqwest` (no Hue crate made the cut: `hue 0.0.1` is a
//! stub, `hue-bridge` is pre-alpha). CLIP v1 is tiny — lights list/get/set
//! plus the link-button pairing flow — and it stays local on the LAN.
//! Bridge Pro serves HTTPS with a self-signed cert, so TLS verification is
//! relaxed *for bridge hosts only* (documented, LAN-scoped, no secrets).
use crate::creds::with_timeout_secs;
use crate::error::DeviceError;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HueLight {
    pub id: String,
    pub name: String,
    pub on: bool,
    pub bri: Option<u8>,
    pub reachable: bool,
}

pub struct HueBridge {
    base: String,
    client: reqwest::Client,
}

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn env_non_empty(name: &str) -> Option<String> {
    clean(std::env::var(name).ok())
}

fn map_http(op: &str, err: reqwest::Error) -> DeviceError {
    if err.is_timeout() {
        DeviceError::Timeout(format!("{op} timed out"))
    } else if err.is_connect() {
        DeviceError::Protocol(format!(
            "{op}: bridge unreachable (wrong IP? set HUE_HTTPS=0 for plain HTTP bridges)"
        ))
    } else {
        DeviceError::Protocol(format!("{op}: {err}"))
    }
}

impl HueBridge {
    /// Explicit args win, `HUE_BRIDGE_IP` / `HUE_USERNAME` are the fallback.
    /// `HUE_HTTPS=0` selects plain HTTP; default is HTTPS (Bridge Pro era).
    pub fn resolve(
        bridge_ip: Option<String>,
        username: Option<String>,
    ) -> Result<Self, DeviceError> {
        let ip = clean(bridge_ip)
            .or_else(|| env_non_empty("HUE_BRIDGE_IP"))
            .ok_or_else(|| {
                DeviceError::InvalidInput(
                    "Hue bridge IP required: --bridge-ip or HUE_BRIDGE_IP".to_string(),
                )
            })?;
        let user = clean(username)
            .or_else(|| env_non_empty("HUE_USERNAME"))
            .ok_or_else(|| {
                DeviceError::Auth(
                    "Hue username required: run hue-pair (press link button) or set HUE_USERNAME"
                        .to_string(),
                )
            })?;
        let https = env_non_empty("HUE_HTTPS").as_deref() != Some("0");
        let scheme = if https { "https" } else { "http" };
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .danger_accept_invalid_certs(https)
            .build()
            .map_err(|e| DeviceError::Protocol(format!("hue http client: {e}")))?;
        Ok(Self {
            base: format!("{scheme}://{ip}/api/{user}"),
            client,
        })
    }

    fn auth_err(body: &str) -> DeviceError {
        DeviceError::Auth(format!(
            "bridge rejected the username (deleted? re-run hue-pair): {body}"
        ))
    }

    pub async fn list_lights(&self) -> Result<Vec<HueLight>, DeviceError> {
        let resp = with_timeout_secs(
            "hue.lights",
            8,
            self.client.get(format!("{}/lights", self.base)).send(),
        )
        .await?
        .map_err(|e| map_http("hue.lights", e))?;
        if resp.status() == reqwest::StatusCode::UNAUTHORIZED
            || resp.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(Self::auth_err(&resp.text().await.unwrap_or_default()));
        }
        let status = resp.status();
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| DeviceError::Protocol(format!("hue.lights decode: {e}")))?;
        if !status.is_success() {
            return Err(DeviceError::Protocol(format!(
                "hue.lights HTTP {status}: {body}"
            )));
        }
        if let Some(err) = body.as_array().and_then(|a| a.first()) {
            return Err(Self::auth_err(&err.to_string()));
        }
        let mut lights = Vec::new();
        if let Some(map) = body.as_object() {
            for (id, lamp) in map {
                let state = lamp.get("state");
                lights.push(HueLight {
                    id: id.clone(),
                    name: lamp
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("?")
                        .to_string(),
                    on: state
                        .and_then(|s| s.get("on"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    bri: state
                        .and_then(|s| s.get("bri"))
                        .and_then(|v| v.as_u64())
                        .map(|b| b.min(254) as u8),
                    reachable: state
                        .and_then(|s| s.get("reachable"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                });
            }
        }
        lights.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(lights)
    }

    pub async fn set_light(
        &self,
        light_id: &str,
        on: bool,
        bri: Option<u8>,
    ) -> Result<(), DeviceError> {
        let id = light_id.trim();
        if id.is_empty() {
            return Err(DeviceError::InvalidInput(
                "light id must not be empty".to_string(),
            ));
        }
        let mut state = serde_json::Map::new();
        state.insert("on".to_string(), serde_json::Value::Bool(on));
        if let Some(b) = bri {
            state.insert("bri".to_string(), serde_json::Value::from(b.clamp(1, 254)));
        }
        let resp = with_timeout_secs(
            "hue.set_light",
            8,
            self.client
                .put(format!("{}/lights/{id}/state", self.base))
                .json(&state)
                .send(),
        )
        .await?
        .map_err(|e| map_http("hue.set_light", e))?;
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| DeviceError::Protocol(format!("hue.set_light decode: {e}")))?;
        if let Some(items) = body.as_array() {
            for item in items {
                if let Some(err) = item.get("error") {
                    return Err(DeviceError::Protocol(format!("hue refused: {err}")));
                }
            }
            return Ok(());
        }
        Err(DeviceError::Protocol(format!(
            "hue.set_light unexpected reply: {body}"
        )))
    }

    /// Link-button pairing: creates a username on the bridge. The caller must
    /// press the physical link button first — otherwise a typed Auth error
    /// says exactly that (CLIP error type 101).
    pub async fn pair(bridge_ip: &str) -> Result<String, DeviceError> {
        let ip = bridge_ip.trim();
        if ip.is_empty() {
            return Err(DeviceError::InvalidInput(
                "bridge IP required: --bridge-ip or HUE_BRIDGE_IP".to_string(),
            ));
        }
        let https = env_non_empty("HUE_HTTPS").as_deref() != Some("0");
        let scheme = if https { "https" } else { "http" };
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .danger_accept_invalid_certs(https)
            .build()
            .map_err(|e| DeviceError::Protocol(format!("hue http client: {e}")))?;
        let resp = with_timeout_secs(
            "hue.pair",
            8,
            client
                .post(format!("{scheme}://{ip}/api"))
                .json(&serde_json::json!({"devicetype": "rust-devices-mcp#desktop"}))
                .send(),
        )
        .await?
        .map_err(|e| map_http("hue.pair", e))?;
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| DeviceError::Protocol(format!("hue.pair decode: {e}")))?;
        let first = body.as_array().and_then(|a| a.first());
        if let Some(ok) = first.and_then(|i| i.get("success")) {
            if let Some(user) = ok.get("username").and_then(|u| u.as_str()) {
                return Ok(user.to_string());
            }
        }
        if let Some(err) = first.and_then(|i| i.get("error")) {
            let kind = err.get("type").and_then(|t| t.as_u64()).unwrap_or(0);
            if kind == 101 {
                return Err(DeviceError::Auth(
                    "press the bridge link button, then retry hue-pair within ~30s".to_string(),
                ));
            }
            return Err(DeviceError::Protocol(format!("hue pairing refused: {err}")));
        }
        Err(DeviceError::Protocol(format!(
            "hue pairing unexpected reply: {body}"
        )))
    }
}
