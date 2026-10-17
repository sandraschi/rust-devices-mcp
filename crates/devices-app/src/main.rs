//! L3 desktop shell (Tauri v2 bridge shape, std + tokio runtime in v0.2).
//!
//! Each `*_command` function is written exactly as a `#[tauri::command]`
//! handler will call it: synchronous from the UI's point of view, in-process,
//! straight into `devices-core::dispatch`. No child process, no local HTTP,
//! no JSON-RPC hop. Wiring the real `tauri` crate + `tauri.conf.json` NSIS
//! bundle is scorecard-tier-4; the function signatures below do not change
//! when it lands.
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unimplemented)]
#![deny(clippy::todo)]

use devices_core::{creds::Vendor, dispatch, no_creds, Command};
use serde_json::Value;
use std::sync::OnceLock;
use tokio::runtime::Runtime;

fn rt() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| match Runtime::new() {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("failed to start async runtime: {e}");
            std::process::exit(1);
        }
    })
}

fn run(cmd: Command) -> Value {
    rt().block_on(dispatch(cmd))
}

/// Tauri command: `plug_get_stats` -- direct in-process call.
fn plug_get_stats_command(device_ip: String) -> Value {
    run(Command::PlugStats {
        vendor: Vendor::Demo,
        device_ip,
        creds: no_creds(),
    })
}

/// Tauri command: `plug_toggle` -- direct in-process call.
fn plug_toggle_command(device_ip: String, state: bool) -> Value {
    run(Command::PlugToggle {
        vendor: Vendor::Demo,
        device_ip,
        state,
        creds: no_creds(),
    })
}

/// Tauri command: `camera_describe` -- direct in-process call.
fn camera_describe_command(device_ip: String) -> Value {
    run(Command::CameraDescribe { device_ip })
}

fn main() {
    // Headless smoke for CI: proves the desktop path without a webview.
    let stats = plug_get_stats_command("192.168.1.50".to_string());
    let toggled = plug_toggle_command("192.168.1.50".to_string(), true);
    let camera = camera_describe_command("192.168.1.51".to_string());
    if let Ok(text) = serde_json::to_string_pretty(&stats) {
        println!("plug.get_stats: {text}");
    }
    if let Ok(text) = serde_json::to_string_pretty(&toggled) {
        println!("plug.toggle: {text}");
    }
    if let Ok(text) = serde_json::to_string_pretty(&camera) {
        println!("camera.describe: {text}");
    }
    // Real shell boots here in tier-4:
    // tauri::Builder::default()
    //     .invoke_handler(tauri::generate_handler![
    //         plug_get_stats_command, plug_toggle_command, camera_describe_command,
    //     ])
    //     .run(tauri::generate_context!())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_path_matches_mcp_dispatch() {
        let via_app = plug_toggle_command("192.168.1.50".to_string(), false);
        let via_core = run(Command::PlugToggle {
            vendor: Vendor::Demo,
            device_ip: "192.168.1.50".to_string(),
            state: false,
            creds: no_creds(),
        });
        assert_eq!(via_app, via_core);
    }
}
