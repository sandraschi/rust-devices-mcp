//! Ring doorbells and alarms, hand-rolled on `reqwest`.
//!
//! No maintained Rust Ring crate exists, so the OAuth + API flow mirrors the
//! reference implementation (`tchellomello/python-ring-doorbell`, read at
//! recon time — not vendored): `LegacyApplicationClient` password grant with
//! `hardware_id` + `2fa-code` headers, refresh-token grant, Bearer API calls
//! with one 401 → refresh → retry, and the `/clients_api` endpoint shapes.
//! Constants (`ring_official_android`, namespace UUID, endpoint paths) are
//! Ring's own — this module speaks their protocol, it doesn't reimplement
//! their crypto.
//!
//! Safety notes: the siren endpoint is deliberately NOT exposed (an acoustic
//! actuator with no interactive confirm over MCP is a footgun). Token cache
//! defaults to `ring_token.cache` (same name as the Python client, so an
//! existing live token is reused, not re-prompted); secrets never hit logs.
use crate::creds::with_timeout_secs;
use crate::error::DeviceError;
use std::time::Duration;

const OAUTH_URL: &str = "https://oauth.ring.com/oauth/token";
const API_BASE: &str = "https://api.ring.com";
const CLIENT_ID: &str = "ring_official_android";
const SCOPE: &str = "client";
const USER_AGENT: &str = "rust-devices-mcp/0.2";
const NAMESPACE_UUID: &str = "379378b0-f747-4b67-a10f-3b13327e8879";

const BASE_KINDS: &[&str] = &["hub.redsky", "security-panel", "base_station_v1"];

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
        .timeout(Duration::from_secs(12))
        .build()
        .map_err(|e| DeviceError::Protocol(format!("ring http client: {e}")))
}

/// Stable hardware id per config, without MAC scraping: the reference lib
/// derives uuid5(namespace, node + UA); we bind namespace + UA + token path,
/// which is equally stable for one operator and leaks nothing about the NIC.
fn hardware_id(token_path: &str) -> String {
    let namespace = uuid::Uuid::parse_str(NAMESPACE_UUID).unwrap_or(uuid::Uuid::NAMESPACE_URL);
    uuid::Uuid::new_v5(&namespace, format!("{USER_AGENT}:{token_path}").as_bytes()).to_string()
}

fn token_path(explicit: Option<String>) -> String {
    clean(explicit)
        .or_else(|| env_non_empty("RING_TOKEN_FILE"))
        .unwrap_or_else(|| "ring_token.cache".to_string())
}

fn read_refresh_token(path: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    json.get("refresh_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

fn persist_tokens(path: &str, access: &str, refresh: Option<&str>) {
    let body = serde_json::json!({
        "access_token": access,
        "refresh_token": refresh,
        "token_type": "Bearer",
    });
    if let Ok(text) = serde_json::to_string(&body) {
        let _ = std::fs::write(path, text);
    }
}

fn classify_oauth(body: &str) -> DeviceError {
    let lower = body.to_ascii_lowercase();
    if lower.contains("2fa")
        || lower.contains("verification")
        || lower.contains("requires2fa")
        || lower.contains("need_verification")
    {
        DeviceError::Auth(
            "Ring 2FA required: check SMS/email for the code, then run ring-auth-2fa --code <code>"
                .to_string(),
        )
    } else if lower.contains("invalid_grant") || lower.contains("invalid_request") {
        DeviceError::Auth(
            "Ring rejected the credentials/token (expired? re-run ring-auth with email + password)"
                .to_string(),
        )
    } else {
        DeviceError::Auth(format!("Ring authentication failed: {body}"))
    }
}

async fn oauth_token(
    http: &reqwest::Client,
    form: &[(&str, &str)],
    hardware: &str,
    code_2fa: Option<&str>,
) -> Result<(String, Option<String>), DeviceError> {
    let mut builder = http
        .post(OAUTH_URL)
        .basic_auth(CLIENT_ID, Some(""))
        .header("User-Agent", USER_AGENT)
        .header("hardware_id", hardware)
        .form(form);
    if let Some(code) = code_2fa {
        builder = builder
            .header("2fa-support", "true")
            .header("2fa-code", code.trim());
    }
    let resp = with_timeout_secs("ring.oauth", 15, builder.send())
        .await?
        .map_err(|e| {
            if e.is_timeout() {
                DeviceError::Timeout("ring.oauth timed out".to_string())
            } else {
                DeviceError::Protocol(format!("ring.oauth: {e}"))
            }
        })?;
    let text = resp
        .text()
        .await
        .map_err(|e| DeviceError::Protocol(format!("ring.oauth body: {e}")))?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|_| classify_oauth(&text))?;
    match json.get("access_token").and_then(|v| v.as_str()) {
        Some(access) if !access.is_empty() => {
            let refresh = json
                .get("refresh_token")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Ok((access.to_string(), refresh))
        }
        _ => Err(classify_oauth(&text)),
    }
}

async fn oauth_refresh(
    http: &reqwest::Client,
    refresh_token: &str,
) -> Result<(String, Option<String>), DeviceError> {
    let resp = with_timeout_secs(
        "ring.refresh",
        15,
        // One Content-Type only: reqwest's .form() already sets
        // application/x-www-form-urlencoded. User-Agent is mandatory: Ring
        // answers 406 with an empty body without one (found live 2026-10-17
        // via transparent refresh errors).
        http.post(OAUTH_URL)
            .basic_auth(CLIENT_ID, Some(""))
            .header("Accept", "application/json")
            .header("User-Agent", USER_AGENT)
            // Body matches the reference lib exactly (grant_type + refresh_token
            // only): the endpoint rejects the call when scope/client_id ride
            // along — verified live 2026-10-17 (minimal body 200, full body 4xx).
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
            ])
            .send(),
    )
    .await?
    .map_err(|e| {
        if e.is_timeout() {
            DeviceError::Timeout("ring.refresh timed out".to_string())
        } else {
            DeviceError::Protocol(format!("ring.refresh: {e}"))
        }
    })?;
    let text = resp
        .text()
        .await
        .map_err(|e| DeviceError::Protocol(format!("ring.refresh body: {e}")))?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|_| classify_oauth(&text))?;
    match json.get("access_token").and_then(|v| v.as_str()) {
        Some(access) if !access.is_empty() => {
            let refresh = json
                .get("refresh_token")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            Ok((access.to_string(), refresh))
        }
        _ => Err(classify_oauth(&text)),
    }
}

/// Bearer token, cheapest path first: explicit/env refresh token, cached
/// refresh token, else password grant (which may demand a 2FA round-trip).
async fn bearer(
    email: Option<String>,
    password: Option<String>,
    token_file: Option<String>,
) -> Result<(String, String), DeviceError> {
    let http = http_client()?;
    let path = token_path(token_file);
    let hardware = hardware_id(&path);

    let mail = clean(email).or_else(|| env_non_empty("RING_EMAIL"));
    let pass = clean(password).or_else(|| env_non_empty("RING_PASSWORD"));

    if let Some(refresh) =
        clean(env_non_empty("RING_REFRESH_TOKEN")).or_else(|| read_refresh_token(&path))
    {
        match oauth_refresh(&http, &refresh).await {
            Ok((access, next)) => {
                persist_tokens(&path, &access, next.as_deref().or(Some(&refresh)));
                return Ok((access, path));
            }
            // A fresh password grant only helps when email + password exist;
            // otherwise report the refresh failure itself, not a misleading
            // "email required".
            Err(DeviceError::Auth(_inner)) if mail.is_some() && pass.is_some() => {}
            Err(e) => return Err(e),
        }
    }

    let mail = mail.ok_or_else(|| {
        DeviceError::Auth(
            "Ring email required: --email or RING_EMAIL (or set RING_REFRESH_TOKEN)".to_string(),
        )
    })?;
    let pass = pass.ok_or_else(|| {
        DeviceError::Auth(
            "Ring password required for first login: --password or RING_PASSWORD".to_string(),
        )
    })?;
    let (access, refresh) = oauth_token(
        &http,
        &[
            ("grant_type", "password"),
            ("username", &mail),
            ("password", &pass),
            ("scope", SCOPE),
            ("client_id", CLIENT_ID),
        ],
        &hardware,
        None,
    )
    .await?;
    persist_tokens(&path, &access, refresh.as_deref());
    Ok((access, path))
}

/// Complete a pending 2FA login after `ring-auth` reported it.
pub async fn submit_2fa(
    code: &str,
    email: Option<String>,
    password: Option<String>,
    token_file: Option<String>,
) -> Result<serde_json::Value, DeviceError> {
    let code = code.trim();
    if code.is_empty() {
        return Err(DeviceError::InvalidInput(
            "2FA code must not be empty".to_string(),
        ));
    }
    let http = http_client()?;
    let path = token_path(token_file);
    let hardware = hardware_id(&path);
    let mail = clean(email)
        .or_else(|| env_non_empty("RING_EMAIL"))
        .ok_or_else(|| {
            DeviceError::Auth("Ring email required: --email or RING_EMAIL".to_string())
        })?;
    let pass = clean(password)
        .or_else(|| env_non_empty("RING_PASSWORD"))
        .ok_or_else(|| {
            DeviceError::Auth("Ring password required: --password or RING_PASSWORD".to_string())
        })?;
    let (access, refresh) = oauth_token(
        &http,
        &[
            ("grant_type", "password"),
            ("username", &mail),
            ("password", &pass),
            ("scope", SCOPE),
            ("client_id", CLIENT_ID),
        ],
        &hardware,
        Some(code),
    )
    .await?;
    persist_tokens(&path, &access, refresh.as_deref());
    Ok(serde_json::json!({"authenticated": true, "token_file": path}))
}

async fn api_get(
    http: &reqwest::Client,
    token: &str,
    path: &str,
    refreshed: &mut bool,
    refresh_token: Option<&str>,
    cache_path: &str,
) -> Result<serde_json::Value, DeviceError> {
    let url = format!("{API_BASE}{path}");
    let call = || {
        http.get(&url)
            .bearer_auth(token)
            .header("User-Agent", USER_AGENT)
            .send()
    };
    let resp = with_timeout_secs("ring.api", 15, call())
        .await?
        .map_err(|e| {
            if e.is_timeout() {
                DeviceError::Timeout(format!("ring {path} timed out"))
            } else {
                DeviceError::Protocol(format!("ring {path}: {e}"))
            }
        })?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED && !*refreshed {
        if let Some(refresh) = refresh_token {
            let (access, next) = oauth_refresh(http, refresh).await?;
            persist_tokens(cache_path, &access, next.as_deref().or(Some(refresh)));
            *refreshed = true;
            let retry = with_timeout_secs("ring.api(retry)", 15, async {
                http.get(&url)
                    .bearer_auth(&access)
                    .header("User-Agent", USER_AGENT)
                    .send()
                    .await
            })
            .await?
            .map_err(|e| DeviceError::Protocol(format!("ring {path} retry: {e}")))?;
            return decode_json(path, retry).await;
        }
    }
    decode_json(path, resp).await
}

async fn decode_json(
    path: &str,
    resp: reqwest::Response,
) -> Result<serde_json::Value, DeviceError> {
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| DeviceError::Protocol(format!("ring {path} body: {e}")))?;
    if !status.is_success() {
        return Err(DeviceError::Protocol(format!(
            "ring {path} HTTP {status}: {text}"
        )));
    }
    serde_json::from_str(&text)
        .map_err(|e| DeviceError::Protocol(format!("ring {path} decode: {e}")))
}

async fn api_put(
    http: &reqwest::Client,
    token: &str,
    path: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, DeviceError> {
    let url = format!("{API_BASE}{path}");
    let resp = with_timeout_secs("ring.api", 15, async {
        http.put(&url)
            .bearer_auth(token)
            .header("User-Agent", USER_AGENT)
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
    })
    .await?
    .map_err(|e| {
        if e.is_timeout() {
            DeviceError::Timeout(format!("ring {path} timed out"))
        } else {
            DeviceError::Protocol(format!("ring {path}: {e}"))
        }
    })?;
    decode_json(path, resp).await
}

async fn api_post(
    http: &reqwest::Client,
    token: &str,
    path: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, DeviceError> {
    let url = format!("{API_BASE}{path}");
    let resp = with_timeout_secs("ring.api", 15, async {
        http.post(&url)
            .bearer_auth(token)
            .header("User-Agent", USER_AGENT)
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
    })
    .await?
    .map_err(|e| {
        if e.is_timeout() {
            DeviceError::Timeout(format!("ring {path} timed out"))
        } else {
            DeviceError::Protocol(format!("ring {path}: {e}"))
        }
    })?;
    decode_json(path, resp).await
}

async fn api_get_bytes(
    http: &reqwest::Client,
    token: &str,
    path: &str,
    refreshed: &mut bool,
    refresh_token: Option<&str>,
    cache_path: &str,
) -> Result<Vec<u8>, DeviceError> {
    let url = format!("{API_BASE}{path}");
    let resp = with_timeout_secs("ring.api", 20, async {
        http.get(&url)
            .bearer_auth(token)
            .header("User-Agent", USER_AGENT)
            .send()
            .await
    })
    .await?
    .map_err(|e| {
        if e.is_timeout() {
            DeviceError::Timeout(format!("ring {path} timed out"))
        } else {
            DeviceError::Protocol(format!("ring {path}: {e}"))
        }
    })?;
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED && !*refreshed {
        if let Some(refresh) = refresh_token {
            let (access, next) = oauth_refresh(http, refresh).await?;
            persist_tokens(cache_path, &access, next.as_deref().or(Some(refresh)));
            *refreshed = true;
            let retry = with_timeout_secs("ring.api(retry)", 20, async {
                http.get(&url)
                    .bearer_auth(&access)
                    .header("User-Agent", USER_AGENT)
                    .send()
                    .await
            })
            .await?
            .map_err(|e| DeviceError::Protocol(format!("ring {path} retry: {e}")))?;
            return bytes_of(path, retry).await;
        }
    }
    bytes_of(path, resp).await
}

async fn bytes_of(path: &str, resp: reqwest::Response) -> Result<Vec<u8>, DeviceError> {
    let status = resp.status();
    if !status.is_success() {
        return Err(DeviceError::Protocol(format!("ring {path} HTTP {status}")));
    }
    resp.bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| DeviceError::Protocol(format!("ring {path} body: {e}")))
}

struct Session {
    http: reqwest::Client,
    token: String,
    refreshed: bool,
    refresh_token: Option<String>,
    cache_path: String,
}

async fn session(
    email: Option<String>,
    password: Option<String>,
    token_file: Option<String>,
) -> Result<Session, DeviceError> {
    let http = http_client()?;
    let path = token_path(token_file);
    // Pre-read the refresh token so a 401 retry can mint a new access token.
    let warm_refresh =
        clean(env_non_empty("RING_REFRESH_TOKEN")).or_else(|| read_refresh_token(&path));
    let (token, cache_path) = bearer(email, password, Some(path.clone())).await?;
    let _ = cache_path;
    Ok(Session {
        http,
        token,
        refreshed: false,
        refresh_token: warm_refresh,
        cache_path: path,
    })
}

impl Session {
    async fn get(&mut self, path: &str) -> Result<serde_json::Value, DeviceError> {
        api_get(
            &self.http,
            &self.token,
            path,
            &mut self.refreshed,
            self.refresh_token.as_deref(),
            &self.cache_path,
        )
        .await
    }

    async fn put(
        &mut self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, DeviceError> {
        api_put(&self.http, &self.token, path, body).await
    }

    async fn post(
        &mut self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, DeviceError> {
        api_post(&self.http, &self.token, path, body).await
    }

    async fn get_bytes(&mut self, path: &str) -> Result<Vec<u8>, DeviceError> {
        api_get_bytes(
            &self.http,
            &self.token,
            path,
            &mut self.refreshed,
            self.refresh_token.as_deref(),
            &self.cache_path,
        )
        .await
    }
}

/// Auth check + device census (doorbells, chimes, cams, alarm gear).
pub async fn status(
    email: Option<String>,
    password: Option<String>,
    token_file: Option<String>,
) -> Result<serde_json::Value, DeviceError> {
    let mut session = session(email, password, token_file).await?;
    let devices = session.get("/clients_api/ring_devices").await?;
    let mut doorbells = 0u64;
    let mut chimes = 0u64;
    let mut cams = 0u64;
    let mut other = 0u64;
    for key in ["doorbots", "authorized_doorbots"] {
        doorbells += devices
            .get(key)
            .and_then(|v| v.as_array())
            .map(|a| a.len() as u64)
            .unwrap_or(0);
    }
    chimes += devices
        .get("chimes")
        .and_then(|v| v.as_array())
        .map(|a| a.len() as u64)
        .unwrap_or(0);
    for key in ["stickup_cams", "authorized_stickup_cams"] {
        cams += devices
            .get(key)
            .and_then(|v| v.as_array())
            .map(|a| a.len() as u64)
            .unwrap_or(0);
    }
    other += devices
        .get("other")
        .and_then(|v| v.as_object())
        .map(|o| o.len() as u64)
        .unwrap_or(0);
    Ok(serde_json::json!({
        "authenticated": true,
        "doorbells": doorbells,
        "chimes": chimes,
        "cameras": cams,
        "alarm_devices": other,
    }))
}

/// Doorbells with health, active dings, and recent history.
pub async fn doorbells(
    email: Option<String>,
    password: Option<String>,
    token_file: Option<String>,
) -> Result<serde_json::Value, DeviceError> {
    let mut session = session(email, password, token_file).await?;
    let devices = session.get("/clients_api/ring_devices").await?;
    let active_dings = session
        .get("/clients_api/dings/active")
        .await
        .unwrap_or(serde_json::Value::Array(Vec::new()));

    let mut out = Vec::new();
    let mut seen = Vec::new();
    for key in ["doorbots", "authorized_doorbots"] {
        if let Some(list) = devices.get(key).and_then(|v| v.as_array()) {
            for doorbell in list {
                let id = doorbell.get("id").and_then(|v| v.as_i64()).unwrap_or(-1);
                let name = doorbell
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("doorbell")
                    .to_string();
                let mut recent = Vec::new();
                if id >= 0 {
                    if let Ok(history) = session
                        .get(&format!("/clients_api/doorbots/{id}/history?limit=3"))
                        .await
                    {
                        if let Some(entries) = history.as_array() {
                            for entry in entries.iter().take(3) {
                                recent.push(serde_json::json!({
                                    "kind": entry.get("kind"),
                                    "created_at": entry.get("created_at"),
                                    "favorite": entry.get("favorite"),
                                }));
                            }
                        }
                    }
                }
                seen.push(id);
                out.push(serde_json::json!({
                    "id": id,
                    "name": name,
                    "kind": doorbell.get("kind"),
                    "recent_events": recent,
                }));
            }
        }
    }
    let _ = seen;
    Ok(serde_json::json!({
        "doorbells": out,
        "active_dings": active_dings,
    }))
}

/// Recursive hunt for an alarm base station: any object whose `kind` is a
/// known base kind contributes (device id, location id). Shape-tolerant by
/// design — the API nests these differently per account.
fn find_base_station(value: &serde_json::Value) -> Option<(String, Option<String>)> {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(kind) = map.get("kind").and_then(|v| v.as_str()) {
                if BASE_KINDS.contains(&kind) {
                    let id = ["device_id", "zid", "id"]
                        .iter()
                        .filter_map(|k| map.get(*k))
                        .filter_map(|v| {
                            v.as_str()
                                .map(|s| s.to_string())
                                .or_else(|| v.as_i64().map(|n| n.to_string()))
                        })
                        .next();
                    let location = map
                        .get("location_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    if let Some(id) = id {
                        return Some((id, location));
                    }
                }
            }
            for child in map.values() {
                if let Some(found) = find_base_station(child) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(items) => {
            for item in items {
                if let Some(found) = find_base_station(item) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn parse_alarm_mode(raw: &str) -> Result<&'static str, DeviceError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "disarmed" | "none" | "off" => Ok("none"),
        "home" | "some" => Ok("some"),
        "away" | "all" | "armed" => Ok("all"),
        _ => Err(DeviceError::InvalidInput(format!(
            "unknown alarm mode '{raw}': expected disarmed|home|away"
        ))),
    }
}

/// Alarm locations with devices and modes (read-only).
pub async fn alarm_status(
    email: Option<String>,
    password: Option<String>,
    token_file: Option<String>,
) -> Result<serde_json::Value, DeviceError> {
    let mut session = session(email, password, token_file).await?;
    let locations = session.get("/clients_api/locations").await?;
    let mut out = Vec::new();
    if let Some(list) = locations.as_array() {
        for location in list {
            let lid = location
                .get("location_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if lid.is_empty() {
                continue;
            }
            let detail = session
                .get(&format!("/clients_api/locations/{lid}"))
                .await
                .unwrap_or(serde_json::Value::Null);
            out.push(serde_json::json!({
                "location_id": lid,
                "name": location.get("name"),
                "mode": detail.get("mode"),
                "devices": detail.get("devices"),
            }));
        }
    }
    Ok(serde_json::Value::Array(out))
}

/// Arm/disarm the alarm. Mode: disarmed|home|away. Siren control is NOT
/// exposed (acoustic actuator, no interactive confirm over MCP — by design).
pub async fn set_alarm_mode(
    mode: &str,
    location_id: Option<String>,
    email: Option<String>,
    password: Option<String>,
    token_file: Option<String>,
) -> Result<serde_json::Value, DeviceError> {
    let api_mode = parse_alarm_mode(mode)?;
    let mut session = session(email, password, token_file).await?;
    let devices = session.get("/clients_api/ring_devices").await?;
    let (base_id, base_location) = find_base_station(&devices).ok_or_else(|| {
        DeviceError::Unsupported("no Ring alarm base station found on this account".to_string())
    })?;
    let lid = clean(location_id).or(base_location).ok_or_else(|| {
        DeviceError::InvalidInput(
            "location id required: --location-id (no location attached to the base station)"
                .to_string(),
        )
    })?;
    let reply = session
        .put(
            &format!("/clients_api/locations/{lid}/devices/{base_id}/mode"),
            &serde_json::json!({"mode": api_mode}),
        )
        .await?;
    Ok(serde_json::json!({
        "location_id": lid,
        "mode": api_mode,
        "updated": true,
        "reply": reply,
    }))
}

/// Doorbell snapshot: trigger capture, poll for a fresh timestamp, download
/// the JPEG. Mirrors the reference lib (`doorbot.async_get_snapshot`):
/// POST timestamps → wait → POST until `timestamp/1000 > request_time` →
/// GET the image for the doorbell id.
pub async fn snapshot_jpeg(
    doorbell_id: i64,
    email: Option<String>,
    password: Option<String>,
    token_file: Option<String>,
) -> Result<Vec<u8>, DeviceError> {
    let mut session = session(email, password, token_file).await?;
    let payload = serde_json::json!({"doorbot_ids": [doorbell_id]});
    session
        .post("/clients_api/snapshots/timestamps", &payload)
        .await?;
    let request_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    for _ in 0..4 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let state = session
            .post("/clients_api/snapshots/timestamps", &payload)
            .await?;
        let fresh = state
            .get("timestamps")
            .and_then(|t| t.as_array())
            .and_then(|a| a.first())
            .and_then(|e| e.get("timestamp"))
            .and_then(|t| t.as_u64())
            .map(|millis| millis / 1000 > request_time)
            .unwrap_or(false);
        if fresh {
            return session
                .get_bytes(&format!("/clients_api/snapshots/image/{doorbell_id}"))
                .await;
        }
    }
    Err(DeviceError::Timeout(
        "ring.snapshot: no fresh frame within ~5s".to_string(),
    ))
}
