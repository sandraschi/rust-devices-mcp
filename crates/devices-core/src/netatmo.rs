//! Netatmo weather stations via the official cloud API (hand-rolled).
//!
//! `netatmo-rs 0.5.0` predates the modern async stack, so OAuth2 refresh +
//! `getstationsdata` ride on `reqwest` directly — the same calls `pyatmo`
//! makes. Credentials resolve explicit-args → env
//! (`NETATMO_CLIENT_ID` / `NETATMO_CLIENT_SECRET` / `NETATMO_REFRESH_TOKEN`).
//! Secrets never appear in errors or logs.
use crate::creds::with_timeout_secs;
use crate::error::DeviceError;
use std::time::Duration;

const TOKEN_URL: &str = "https://api.netatmo.com/oauth2/token";
const STATIONS_URL: &str = "https://api.netatmo.com/api/getstationsdata";

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
        .map_err(|e| DeviceError::Protocol(format!("netatmo http client: {e}")))
}

/// Full stations payload, compacted per station/module but otherwise verbatim
/// (`dashboard_data` is the payload agents actually want — no lossy mapping).
pub async fn stations(
    client_id: Option<String>,
    client_secret: Option<String>,
    refresh_token: Option<String>,
) -> Result<serde_json::Value, DeviceError> {
    let id = clean(client_id)
        .or_else(|| env_non_empty("NETATMO_CLIENT_ID"))
        .ok_or_else(|| {
            DeviceError::Auth(
                "Netatmo client id required: --client-id or NETATMO_CLIENT_ID".to_string(),
            )
        })?;
    let secret = clean(client_secret)
        .or_else(|| env_non_empty("NETATMO_CLIENT_SECRET"))
        .ok_or_else(|| {
            DeviceError::Auth(
                "Netatmo client secret required: --client-secret or NETATMO_CLIENT_SECRET"
                    .to_string(),
            )
        })?;
    let refresh = clean(refresh_token)
        .or_else(|| env_non_empty("NETATMO_REFRESH_TOKEN"))
        .ok_or_else(|| {
            DeviceError::Auth(
                "Netatmo refresh token required: --refresh-token or NETATMO_REFRESH_TOKEN"
                    .to_string(),
            )
        })?;

    let http = http_client()?;
    let token_resp = with_timeout_secs(
        "netatmo.refresh",
        12,
        http.post(TOKEN_URL)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh.as_str()),
                ("client_id", id.as_str()),
                ("client_secret", secret.as_str()),
            ])
            .send(),
    )
    .await?
    .map_err(|e| {
        if e.is_timeout() {
            DeviceError::Timeout("netatmo.refresh timed out".to_string())
        } else {
            DeviceError::Protocol(format!("netatmo.refresh: {e}"))
        }
    })?;
    if !token_resp.status().is_success() {
        let body = token_resp.text().await.unwrap_or_default();
        if body.contains("invalid_grant") {
            return Err(DeviceError::Auth(
                "Netatmo refresh token rejected (revoked? re-authorize the app)".to_string(),
            ));
        }
        return Err(DeviceError::Auth(format!(
            "Netatmo token refresh failed: {body}"
        )));
    }
    let token: serde_json::Value = token_resp
        .json()
        .await
        .map_err(|e| DeviceError::Protocol(format!("netatmo token decode: {e}")))?;
    let access = token
        .get("access_token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            DeviceError::Protocol("Netatmo token reply carried no access_token".to_string())
        })?;

    let data_resp = with_timeout_secs(
        "netatmo.stations",
        12,
        http.get(STATIONS_URL).bearer_auth(access).send(),
    )
    .await?
    .map_err(|e| {
        if e.is_timeout() {
            DeviceError::Timeout("netatmo.stations timed out".to_string())
        } else {
            DeviceError::Protocol(format!("netatmo.stations: {e}"))
        }
    })?;
    if !data_resp.status().is_success() {
        return Err(DeviceError::Protocol(format!(
            "netatmo.stations HTTP {}: {}",
            data_resp.status(),
            data_resp.text().await.unwrap_or_default()
        )));
    }
    let body: serde_json::Value = data_resp
        .json()
        .await
        .map_err(|e| DeviceError::Protocol(format!("netatmo.stations decode: {e}")))?;
    Ok(compact_stations(&body))
}

fn compact_stations(body: &serde_json::Value) -> serde_json::Value {
    let mut out = Vec::new();
    let devices = body
        .get("body")
        .and_then(|b| b.get("devices"))
        .and_then(|d| d.as_array());
    let empty = Vec::new();
    for station in devices.unwrap_or(&empty) {
        let mut modules = Vec::new();
        if let Some(list) = station.get("modules").and_then(|m| m.as_array()) {
            for module in list {
                modules.push(serde_json::json!({
                    "module_name": module.get("module_name"),
                    "type": module.get("type"),
                    "data_type": module.get("data_type"),
                    "dashboard_data": module.get("dashboard_data"),
                    "reachable": module.get("reachable"),
                }));
            }
        }
        out.push(serde_json::json!({
            "station_name": station.get("station_name"),
            "type": station.get("type"),
            "reachable": station.get("reachable"),
            "dashboard_data": station.get("dashboard_data"),
            "modules": modules,
        }));
    }
    serde_json::Value::Array(out)
}
