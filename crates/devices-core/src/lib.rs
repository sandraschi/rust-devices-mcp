//! L1 device core: protocols, state machine, command bus.
//!
//! UI runtimes (Tauri/egui), transports (MCP stdio), and CLIs are forbidden
//! from touching device state directly. Everything routes through
//! [`Command::dispatch`] with typed params, so the MCP server is a thin frame
//! around one async function — now spanning six vendors: Tapo/Kasa/Shelly
//! plugs, Hue lighting, Netatmo weather, Ring doorbells + alarms.
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unimplemented)]
#![deny(clippy::todo)]

pub mod cam_onvif;
pub mod cam_tapo;
pub mod cam_url;
pub mod cam_usb;
pub mod camera;
pub mod creds;
pub mod error;
pub mod hue;
pub mod import;
pub mod netatmo;
pub mod plug;
pub mod plug_kasa;
pub mod plug_shelly;
pub mod plug_tapo;
pub mod ring;
pub mod weather;

use creds::{Credentials, Vendor};
use serde_json::{json, Value};

/// Unique string IDs exposed over MCP `tools/list` and the CLI.
pub const CMD_STATUS: &str = "status";
pub const CMD_PLUG_STATS: &str = "plug.get_stats";
pub const CMD_PLUG_TOGGLE: &str = "plug.toggle";
pub const CMD_CAMERA_DESCRIBE: &str = "camera.describe";
pub const CMD_CAMERA_SNAPSHOT: &str = "camera.snapshot";
pub const CMD_HUE_LIGHTS: &str = "hue.list_lights";
pub const CMD_HUE_SET: &str = "hue.set_light";
pub const CMD_HUE_PAIR: &str = "hue.pair";
pub const CMD_NETATMO: &str = "netatmo.stations";
pub const CMD_RING_STATUS: &str = "ring.status";
pub const CMD_RING_DOORBELLS: &str = "ring.doorbells";
pub const CMD_RING_ALARM: &str = "ring.alarm";
pub const CMD_RING_AUTH: &str = "ring.auth";
pub const CMD_RING_AUTH_2FA: &str = "ring.auth_2fa";
pub const CMD_WEATHER: &str = "weather.now";

/// `None` credentials (rely on env) for terse call sites.
pub fn no_creds() -> Credentials {
    Credentials::default()
}

/// Bounded command bus. New capability = new variant + dispatch arm + test.
#[derive(Debug, Clone)]
pub enum Command {
    Status,
    PlugStats {
        vendor: Vendor,
        device_ip: String,
        creds: Credentials,
    },
    PlugToggle {
        vendor: Vendor,
        device_ip: String,
        state: bool,
        creds: Credentials,
    },
    CameraDescribe {
        device_ip: String,
    },
    CameraSnapshot {
        source: camera::CamSource,
        device_ip: String,
        model: Option<String>,
        username: Option<String>,
        password: Option<String>,
        onvif_port: Option<u16>,
        device_index: Option<u32>,
        url: Option<String>,
        out_path: Option<String>,
        ring_token_file: Option<String>,
    },
    HueLights {
        bridge_ip: Option<String>,
        username: Option<String>,
    },
    HueSetLight {
        bridge_ip: Option<String>,
        username: Option<String>,
        light_id: String,
        on: bool,
        bri: Option<u8>,
    },
    HuePair {
        bridge_ip: String,
    },
    NetatmoStations {
        client_id: Option<String>,
        client_secret: Option<String>,
        refresh_token: Option<String>,
    },
    RingStatus {
        email: Option<String>,
        password: Option<String>,
        token_file: Option<String>,
    },
    RingDoorbells {
        email: Option<String>,
        password: Option<String>,
        token_file: Option<String>,
    },
    /// `mode: None` reads alarm state; `Some` arms/disarms.
    RingAlarm {
        mode: Option<String>,
        location_id: Option<String>,
        email: Option<String>,
        password: Option<String>,
        token_file: Option<String>,
    },
    RingAuth {
        email: Option<String>,
        password: Option<String>,
        token_file: Option<String>,
    },
    RingAuth2fa {
        code: String,
        email: Option<String>,
        password: Option<String>,
        token_file: Option<String>,
    },
    Weather {
        latitude: Option<f64>,
        longitude: Option<f64>,
    },
}

fn encode<T: serde::Serialize>(op: &str, result: Result<T, error::DeviceError>) -> Value {
    match result {
        Ok(value) => serde_json::to_value(&value)
            .unwrap_or_else(|e| json!({"error": format!("{op} encode: {e}")})),
        Err(e) => json!({"error": e.to_string()}),
    }
}

/// Dispatch one command headlessly. Never panics; failures become JSON values
/// so the MCP layer frames them without branching.
pub async fn dispatch(cmd: Command) -> Value {
    let plug = plug::PlugController::new();
    let cam = camera::CameraController::new();
    match cmd {
        Command::Status => json!({
            "app": "rust-devices-mcp",
            "version": env!("CARGO_PKG_VERSION"),
            "mode": "pure-rust",
            "commands": [
                CMD_STATUS, CMD_PLUG_STATS, CMD_PLUG_TOGGLE,
                CMD_CAMERA_DESCRIBE, CMD_CAMERA_SNAPSHOT,
                CMD_HUE_LIGHTS, CMD_HUE_SET, CMD_HUE_PAIR, CMD_NETATMO,
                CMD_RING_STATUS, CMD_RING_DOORBELLS, CMD_RING_ALARM,
                CMD_RING_AUTH, CMD_RING_AUTH_2FA, CMD_WEATHER,
            ],
            "agent_contract": "CLI-first: every command supports --json; `mcp` subcommand mirrors the same dispatch over stdio",
        }),
        Command::PlugStats {
            vendor,
            device_ip,
            creds,
        } => encode("plug.get_stats", plug.get_stats(vendor, &device_ip, &creds).await),
        Command::PlugToggle {
            vendor,
            device_ip,
            state,
            creds,
        } => match plug.toggle(vendor, &device_ip, state, &creds).await {
            Ok(()) => json!({"device_ip": device_ip, "vendor": vendor.as_str(), "state": state, "updated": true}),
            Err(e) => json!({"error": e.to_string()}),
        },
        Command::CameraDescribe { device_ip } => {
            encode("camera.describe", cam.describe(&device_ip))
        }
        Command::CameraSnapshot {
            source,
            device_ip,
            model,
            username,
            password,
            onvif_port,
            device_index,
            url,
            out_path,
            ring_token_file,
        } => {
            let result = camera::snapshot(
                source,
                &device_ip,
                model,
                username,
                password,
                onvif_port,
                device_index,
                url,
                out_path,
                None,
                ring_token_file,
            )
            .await;
            encode("camera.snapshot", result)
        }
        Command::HueLights {
            bridge_ip,
            username,
        } => {
            let result = async {
                let bridge = hue::HueBridge::resolve(bridge_ip, username)?;
                bridge.list_lights().await
            }
            .await;
            encode("hue.list_lights", result)
        }
        Command::HueSetLight {
            bridge_ip,
            username,
            light_id,
            on,
            bri,
        } => {
            let result = async {
                let bridge = hue::HueBridge::resolve(bridge_ip, username)?;
                bridge.set_light(&light_id, on, bri).await?;
                Ok(serde_json::json!({"light_id": light_id, "on": on, "bri": bri, "updated": true}))
            }
            .await;
            encode("hue.set_light", result)
        }
        Command::HuePair { bridge_ip } => {
            encode("hue.pair", hue::HueBridge::pair(&bridge_ip).await.map(|username| {
                serde_json::json!({"username": username, "next": "set HUE_USERNAME to this value"})
            }))
        }
        Command::NetatmoStations {
            client_id,
            client_secret,
            refresh_token,
        } => encode(
            "netatmo.stations",
            netatmo::stations(client_id, client_secret, refresh_token).await,
        ),
        Command::RingStatus {
            email,
            password,
            token_file,
        } => encode(
            "ring.status",
            ring::status(email, password, token_file).await,
        ),
        Command::RingDoorbells {
            email,
            password,
            token_file,
        } => encode(
            "ring.doorbells",
            ring::doorbells(email, password, token_file).await,
        ),
        Command::RingAlarm {
            mode,
            location_id,
            email,
            password,
            token_file,
        } => match mode {
            None => encode(
                "ring.alarm",
                ring::alarm_status(email, password, token_file).await,
            ),
            Some(m) => encode(
                "ring.alarm",
                ring::set_alarm_mode(&m, location_id, email, password, token_file).await,
            ),
        },
        Command::RingAuth {
            email,
            password,
            token_file,
        } => {
            // Auth check that also proves the token path end to end.
            let result = ring::status(email, password, token_file)
                .await
                .map(|summary| {
                    serde_json::json!({"authenticated": true, "summary": summary})
                });
            encode("ring.auth", result)
        }
        Command::RingAuth2fa {
            code,
            email,
            password,
            token_file,
        } => encode(
            "ring.auth_2fa",
            ring::submit_2fa(&code, email, password, token_file).await,
        ),
        Command::Weather {
            latitude,
            longitude,
        } => encode("weather.now", weather::now(latitude, longitude).await),
    }
}

/// Strict `DeviceError` display helper for tests (avoids asserting on strings).
pub fn is_invalid_input(value: &Value) -> bool {
    value
        .get("error")
        .and_then(|e| e.as_str())
        .unwrap_or("")
        .contains("invalid input")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::DeviceError;

    #[tokio::test]
    async fn status_lists_all_commands() {
        let v = dispatch(Command::Status).await;
        let cmds = v
            .get("commands")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(cmds.len(), 15);
    }

    #[tokio::test]
    async fn hostile_empty_ip_becomes_error_value_not_panic() {
        let v = dispatch(Command::PlugStats {
            vendor: Vendor::Demo,
            device_ip: String::new(),
            creds: no_creds(),
        })
        .await;
        assert!(is_invalid_input(&v));
        let _ = DeviceError::Offline("test".to_string()).to_string();
    }

    #[tokio::test]
    async fn toggle_round_trip() {
        let v = dispatch(Command::PlugToggle {
            vendor: Vendor::Demo,
            device_ip: "192.168.1.50".to_string(),
            state: false,
            creds: no_creds(),
        })
        .await;
        assert!(v.get("updated").and_then(|u| u.as_bool()).unwrap_or(false));
    }

    #[tokio::test]
    async fn bad_alarm_mode_is_typed_error_value() {
        let v = dispatch(Command::RingAlarm {
            mode: Some("siren-everything".to_string()),
            location_id: None,
            email: None,
            password: None,
            token_file: None,
        })
        .await;
        assert!(is_invalid_input(&v));
    }

    #[tokio::test]
    async fn bad_vendor_is_typed_error_at_parse() {
        assert!(Vendor::parse("toaster").is_err());
    }
}
