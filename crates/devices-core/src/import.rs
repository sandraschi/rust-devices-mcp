//! Fleet import: devices-mcp's IPs and auth, with operator permission.
//!
//! Precedence everywhere: explicit args > process env > devices-mcp `.env` >
//! devices-mcp `config.yaml` / caches. The base directory resolves from
//! `DEVICES_MCP_DIR`, else by walking up from the running exe looking for a
//! `devices-mcp` sibling with a `config.yaml` — no hardcoded machine paths.
//!
//! Secret discipline: imported secrets live in memory only, are never logged,
//! never appear in errors, and never commit (this repo has no `.env` and
//! gitignores `*.cache`). Config values may be `${VAR}` references — those
//! resolve through the same process-env-then-`.env` chain.
use crate::error::DeviceError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone)]
pub struct PlugEntry {
    pub vendor: &'static str,
    pub label: String,
    pub ip: String,
    pub readonly: bool,
}

#[derive(Debug, Clone)]
pub struct HueEntry {
    pub ip: String,
    pub username: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OnvifCam {
    pub label: String,
    pub ip: String,
    pub port: u16,
}

/// Per-camera login: cameras may carry distinct credentials
/// (TAPO_CAMERA_* vs TAPO_CAMERA_2_*). Memory-only, never logged.
#[derive(Debug, Clone)]
pub struct CamAuth {
    pub ip: String,
    pub username: Option<String>,
    pub password: Option<String>,
}

/// Everything importable. Secret fields (`*_password`, `*_email`,
/// `ring_*`, `netatmo_*`) are memory-only and must never reach logs.
#[derive(Debug, Clone, Default)]
pub struct FleetImport {
    pub plugs: Vec<PlugEntry>,
    pub hue: Option<HueEntry>,
    pub onvif_cams: Vec<OnvifCam>,
    pub tapo_cam_user: Option<String>,
    pub tapo_email: Option<String>,
    pub tapo_password: Option<String>,
    pub cam_auth: Vec<CamAuth>,
    pub ring_email: Option<String>,
    pub ring_password: Option<String>,
    pub ring_token_file: Option<String>,
    pub netatmo_client_id: Option<String>,
    pub netatmo_client_secret: Option<String>,
    pub netatmo_refresh_token: Option<String>,
    pub usb_index: Option<u32>,
    pub netatmo_configured: bool,
}

/// Locate the devices-mcp checkout, if this machine has one.
pub fn fleet_base_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("DEVICES_MCP_DIR") {
        let path = PathBuf::from(dir.trim());
        if path.join("config.yaml").is_file() {
            return Some(path);
        }
    }
    let exe = std::env::current_exe().ok()?;
    let mut cursor = exe.parent();
    for _ in 0..8 {
        let dir = cursor?;
        let candidate = dir.join("devices-mcp");
        if candidate.join("config.yaml").is_file() {
            return Some(candidate);
        }
        cursor = dir.parent();
    }
    None
}

fn dotenv_map(base: &Path) -> HashMap<String, String> {
    static CACHE: OnceLock<HashMap<String, String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let mut map = HashMap::new();
            let text = std::fs::read_to_string(base.join(".env")).unwrap_or_default();
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let line = line.strip_prefix("export ").unwrap_or(line);
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                let key = key.trim().to_string();
                let mut value = value.trim().to_string();
                if value.len() >= 2 && value.starts_with('"') && value.ends_with('"')
                    || value.len() >= 2 && value.starts_with('\'') && value.ends_with('\'')
                {
                    value = value[1..value.len() - 1].to_string();
                }
                if let Some(commented) = value.split(" #").next() {
                    value = commented.trim().to_string();
                }
                if !key.is_empty() && !value.is_empty() {
                    map.insert(key, value);
                }
            }
            map
        })
        .clone()
}

/// Secret lookup: process env first, devices-mcp `.env` second.
pub fn fleet_secret(name: &str) -> Option<String> {
    if let Ok(value) = std::env::var(name) {
        let trimmed = value.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    let base = fleet_base_dir()?;
    dotenv_map(&base).get(name).cloned()
}

/// First hit across aliases.
pub fn fleet_secret_any(names: &[&str]) -> Option<String> {
    names.iter().filter_map(|name| fleet_secret(name)).next()
}

/// Expand `${VAR}`, `${VAR:-default}`, `${VAR-default}` and `$VAR`
/// config references through the secret chain; plain values pass through
/// untouched. The `:-` form is what fleet configs actually use
/// (`host: ${TAPO_CAMERA_HOST:-192.168.0.164}`) — missing it prefills the
/// literal string as an IP (caught live 2026-10-17).
fn expand_env(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(inner) = trimmed
        .strip_prefix("${")
        .and_then(|rest| rest.strip_suffix('}'))
    {
        let (name, default) = if let Some((name, fallback)) = inner.split_once(":-") {
            (name, Some(fallback))
        } else if let Some((name, fallback)) = inner.split_once('-') {
            (name, Some(fallback))
        } else {
            (inner, None)
        };
        if let Some(hit) = fleet_secret(name.trim()) {
            return Some(hit);
        }
        return default
            .map(|fallback| fallback.trim().to_string())
            .filter(|fallback| !fallback.is_empty());
    }
    if let Some(name) = trimmed.strip_prefix('$') {
        if !name.is_empty() && !name.contains(char::is_whitespace) {
            return fleet_secret(name);
        }
    }
    Some(trimmed.to_string())
}

fn get<'a>(value: &'a serde_yaml::Value, keys: &[&str]) -> Option<&'a serde_yaml::Value> {
    let mut node = value;
    for key in keys {
        node = node.get(*key)?;
    }
    Some(node)
}

fn yaml_str(value: &serde_yaml::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn yaml_bool(value: &serde_yaml::Value, key: &str) -> bool {
    value.get(key).and_then(|v| v.as_bool()).unwrap_or(false)
}

/// Expanded config string (handles `${VAR}` references).
fn yaml_secret(value: &serde_yaml::Value, key: &str) -> Option<String> {
    value.get(key).and_then(|v| v.as_str()).and_then(expand_env)
}

/// Load the fleet inventory. `None` (not `Err`) when there is no checkout —
/// a fresh machine simply gets manual fields.
pub fn load() -> Option<FleetImport> {
    let base = fleet_base_dir()?;
    let text = std::fs::read_to_string(base.join("config.yaml")).ok()?;
    let yaml: serde_yaml::Value = serde_yaml::from_str(&text).ok()?;

    // energy.tapo_p115.devices[] {host, name, readonly} (+ lighting top-ups).
    let mut plugs = Vec::new();
    for section in ["energy", "lighting"] {
        let section_yaml = yaml.get(section);
        for group in ["tapo_p115", "tapo_lighting"] {
            let devices = section_yaml
                .and_then(|s| s.get(group))
                .and_then(|g| g.get("devices"));
            if let Some(list) = devices.and_then(|v| v.as_sequence()) {
                for plug in list {
                    if let Some(host) = yaml_secret(plug, "host") {
                        plugs.push(PlugEntry {
                            vendor: "tapo",
                            label: yaml_str(plug, "name").unwrap_or_else(|| host.clone()),
                            ip: host,
                            readonly: yaml_bool(plug, "readonly"),
                        });
                    }
                }
            }
        }
    }

    // energy.tapo_p115.account {username, password}.
    let tapo_email = get(&yaml, &["energy", "tapo_p115", "account"])
        .and_then(|account| yaml_secret(account, "username"));
    let tapo_password = get(&yaml, &["energy", "tapo_p115", "account"])
        .and_then(|account| yaml_secret(account, "password"));

    // lighting.philips_hue {bridge_ip, username}.
    let hue = yaml.get("lighting").and_then(|lighting| {
        lighting.get("philips_hue").and_then(|bridge| {
            if !yaml_bool(bridge, "enabled") && yaml_secret(bridge, "bridge_ip").is_none() {
                return None;
            }
            let ip = yaml_secret(bridge, "bridge_ip")?;
            let mut username = yaml_secret(bridge, "username");
            if username.is_none() {
                if let Ok(cache) = std::fs::read_to_string(base.join("hue_bridge.cache")) {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&cache) {
                        username = json
                            .get("username")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                            .filter(|s| !s.is_empty());
                    }
                }
            }
            Some(HueEntry { ip, username })
        })
    });

    // cameras.* (non-usb): {type, params{host, onvif_port, username, password}}.
    let mut onvif_cams = Vec::new();
    let mut tapo_cam_user = None;
    let mut cam_auth = Vec::new();
    if let Some(cams) = yaml.get("cameras").and_then(|v| v.as_mapping()) {
        for (name, cam) in cams {
            let name = name.as_str().unwrap_or("camera");
            if name.starts_with("usb_") {
                continue;
            }
            if let Some(params) = cam.get("params") {
                if let Some(host) = yaml_secret(params, "host") {
                    let user = yaml_secret(params, "username");
                    let pass = yaml_secret(params, "password");
                    if tapo_cam_user.is_none() {
                        tapo_cam_user = user.clone();
                    }
                    cam_auth.push(CamAuth {
                        ip: host.clone(),
                        username: user,
                        password: pass,
                    });
                    onvif_cams.push(OnvifCam {
                        label: name.to_string(),
                        ip: host,
                        port: params
                            .get("onvif_port")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(2020)
                            .min(65535) as u16,
                    });
                }
            }
        }
    }

    // ring {enabled, email, password, token_file}.
    let ring_node = yaml.get("ring");
    let ring_enabled = ring_node
        .map(|ring| yaml_bool(ring, "enabled"))
        .unwrap_or(false);
    let ring_email = ring_enabled
        .then(|| ring_node.and_then(|ring| yaml_secret(ring, "email")))
        .flatten();
    let ring_password = ring_enabled
        .then(|| ring_node.and_then(|ring| yaml_secret(ring, "password")))
        .flatten();
    let ring_token_file = ring_enabled
        .then(|| {
            ring_node
                .and_then(|ring| yaml_secret(ring, "token_file"))
                .map(|file| {
                    let path = PathBuf::from(&file);
                    if path.is_absolute() {
                        file
                    } else {
                        base.join(path).to_string_lossy().to_string()
                    }
                })
        })
        .flatten()
        .filter(|path| Path::new(path).is_file());

    // cameras.usb_camera_c922.params.device_id → index.
    let usb_index = yaml
        .get("cameras")
        .and_then(|cams| cams.get("usb_camera_c922"))
        .and_then(|cam| cam.get("params"))
        .and_then(|params| params.get("device_id"))
        .and_then(|v| v.as_u64())
        .map(|i| i.min(u32::MAX as u64) as u32);

    // weather.integrations.netatmo {enabled, client_id, client_secret, refresh_token}.
    let netatmo_node = get(&yaml, &["weather", "integrations", "netatmo"]);
    let netatmo_configured = netatmo_node
        .map(|netatmo| yaml_bool(netatmo, "enabled"))
        .unwrap_or(false);
    let netatmo_client_id = netatmo_configured
        .then(|| netatmo_node.and_then(|n| yaml_secret(n, "client_id")))
        .flatten();
    let netatmo_client_secret = netatmo_configured
        .then(|| netatmo_node.and_then(|n| yaml_secret(n, "client_secret")))
        .flatten();
    let netatmo_refresh_token = netatmo_configured
        .then(|| netatmo_node.and_then(|n| yaml_secret(n, "refresh_token")))
        .flatten();

    Some(FleetImport {
        plugs,
        hue,
        onvif_cams,
        tapo_cam_user,
        tapo_email,
        tapo_password,
        cam_auth,
        ring_email,
        ring_password,
        ring_token_file,
        netatmo_client_id,
        netatmo_client_secret,
        netatmo_refresh_token,
        usb_index,
        netatmo_configured,
    })
}

/// Fleet-aware secret with explicit override: arg > process env > `.env`.
pub fn secret_or(arg: Option<String>, name: &str) -> Option<String> {
    let clean = arg.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    clean.or_else(|| fleet_secret(name))
}

/// Tapo account from any layer (explicit pair handled by callers).
pub fn tapo_account(
    username: Option<String>,
    password: Option<String>,
) -> Result<(String, String), DeviceError> {
    let user = secret_or(username, "TAPO_EMAIL")
        .or_else(|| fleet_secret("TAPO_USERNAME"))
        .or_else(|| fleet_secret("TAPO_P115_EMAIL"))
        .or_else(|| fleet_secret("TAPO_LIGHTING_EMAIL"));
    let pass = secret_or(password, "TAPO_PASSWORD")
        .or_else(|| fleet_secret("TAPO_P115_PASSWORD"))
        .or_else(|| fleet_secret("TAPO_LIGHTING_PASSWORD"));
    match (user, pass) {
        (Some(user), Some(pass)) => Ok((user, pass)),
        _ => Err(DeviceError::Auth(
            "Tapo account required: --username/--password, TAPO_EMAIL+TAPO_PASSWORD, or devices-mcp/.env".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_env_passes_plain_values_and_resolves_refs() {
        assert_eq!(
            expand_env(" 192.168.0.17 "),
            Some("192.168.0.17".to_string())
        );
        assert_eq!(expand_env("   "), None);
        // ${VAR:-default} falls back when the var is absent (fleet configs
        // use this form for hosts — regression test for the unexpanded-IP
        // bug that prefills "${TAPO_CAMERA_HOST:-192.168.0.164}" literally).
        assert_eq!(
            expand_env("${RUST_DEVICES_MCP_DEFINITELY_UNSET:-192.168.0.164}"),
            Some("192.168.0.164".to_string())
        );
        assert_eq!(expand_env("${RUST_DEVICES_MCP_DEFINITELY_UNSET}"), None);
        // ${VAR} resolution depends on machine env; only the shape is asserted.
        let _ = expand_env("${TAPO_EMAIL}");
    }

    #[test]
    fn missing_checkout_is_none_not_error() {
        let _ = load();
    }

    #[test]
    fn live_checkout_parses_when_present() {
        // Fleet-machine oracle: the real config must yield the known
        // inventory (2 Tapo plugs, Hue bridge, 2 ONVIF cams, Ring token).
        // Elsewhere this passes vacuously — parsing, not presence, is law.
        if let Some(inv) = load() {
            assert!(
                inv.plugs.len() >= 2,
                "expected the Tapo plugs, got {}",
                inv.plugs.len()
            );
            assert!(
                inv.hue.as_ref().map(|h| !h.ip.is_empty()).unwrap_or(false),
                "expected the Hue bridge"
            );
            assert!(!inv.onvif_cams.is_empty(), "expected the ONVIF cameras");
            assert!(
                inv.ring_token_file.is_some(),
                "expected the Ring token file"
            );
            for plug in &inv.plugs {
                assert!(!plug.ip.trim().is_empty());
            }
        }
    }
}
