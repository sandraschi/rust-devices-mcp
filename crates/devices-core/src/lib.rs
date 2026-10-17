//! L1 device core: protocols, state machine, command bus.
//!
//! UI runtimes (Tauri/egui), transports (MCP stdio), and CLIs are forbidden
//! from touching `plug` / `camera` state directly. Everything routes through
//! [`Command::dispatch`] with typed params, so the MCP server is a thin frame
//! around this function.
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unimplemented)]
#![deny(clippy::todo)]

pub mod camera;
pub mod error;
pub mod plug;

use serde_json::{json, Value};

/// Unique string IDs exposed over MCP `tools/list` and the CLI.
pub const CMD_STATUS: &str = "status";
pub const CMD_PLUG_STATS: &str = "plug.get_stats";
pub const CMD_PLUG_TOGGLE: &str = "plug.toggle";
pub const CMD_CAMERA_DESCRIBE: &str = "camera.describe";
pub const CMD_CAMERA_SNAPSHOT: &str = "camera.snapshot_meta";

/// Bounded command bus. New capability = new variant + dispatch arm + test.
#[derive(Debug, Clone)]
pub enum Command {
    Status,
    PlugStats { device_ip: String },
    PlugToggle { device_ip: String, state: bool },
    CameraDescribe { device_ip: String },
    CameraSnapshot { device_ip: String },
}

/// Dispatch one command headlessly. Never panics; failures become JSON values
/// so the MCP layer can frame them without branching.
pub fn dispatch(cmd: Command) -> Value {
    let plug = plug::PlugController::new();
    let cam = camera::CameraController::new();
    match cmd {
        Command::Status => json!({
            "app": "rust-devices-mcp",
            "version": env!("CARGO_PKG_VERSION"),
            "mode": "pure-rust",
            "commands": [CMD_STATUS, CMD_PLUG_STATS, CMD_PLUG_TOGGLE, CMD_CAMERA_DESCRIBE, CMD_CAMERA_SNAPSHOT],
            "agent_contract": "CLI-first: every command supports --json; `mcp` subcommand mirrors the same dispatch over stdio",
        }),
        Command::PlugStats { device_ip } => match plug.get_stats(&device_ip) {
            Ok(stats) => match serde_json::to_value(&stats) {
                Ok(v) => v,
                Err(e) => json!({"error": format!("encode power stats: {e}")}),
            },
            Err(e) => json!({"error": e.to_string()}),
        },
        Command::PlugToggle { device_ip, state } => match plug.toggle(&device_ip, state) {
            Ok(()) => json!({"device_ip": device_ip, "state": state, "updated": true}),
            Err(e) => json!({"error": e.to_string()}),
        },
        Command::CameraDescribe { device_ip } => match cam.describe(&device_ip) {
            Ok(info) => match serde_json::to_value(&info) {
                Ok(v) => v,
                Err(e) => json!({"error": format!("encode camera info: {e}")}),
            },
            Err(e) => json!({"error": e.to_string()}),
        },
        Command::CameraSnapshot { device_ip } => match cam.snapshot_meta(&device_ip) {
            Ok(meta) => match serde_json::to_value(&meta) {
                Ok(v) => v,
                Err(e) => json!({"error": format!("encode snapshot meta: {e}")}),
            },
            Err(e) => json!({"error": e.to_string()}),
        },
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

    #[test]
    fn status_lists_all_commands() {
        let v = dispatch(Command::Status);
        let cmds = v
            .get("commands")
            .and_then(|c| c.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(cmds.len(), 5);
    }

    #[test]
    fn hostile_empty_ip_becomes_error_value_not_panic() {
        let v = dispatch(Command::PlugStats {
            device_ip: String::new(),
        });
        assert!(is_invalid_input(&v));
        // Confirm the Display impl is reachable without unwrapping.
        let _ = DeviceError::Offline("test".to_string()).to_string();
    }

    #[test]
    fn toggle_round_trip() {
        let v = dispatch(Command::PlugToggle {
            device_ip: "192.168.1.50".to_string(),
            state: false,
        });
        assert!(v.get("updated").and_then(|u| u.as_bool()).unwrap_or(false));
    }
}
