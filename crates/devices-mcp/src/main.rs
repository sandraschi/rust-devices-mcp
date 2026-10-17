//! L2 binary: CLI-first + MCP `mcp` subcommand over stdio.
//!
//! Stdout is pure JSON-RPC. Every log line goes to stderr via `eprintln!`.
//! The MCP layer never touches hardware state; it dispatches `Command` IDs
//! into `devices-core` and frames the resulting `serde_json::Value`.
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unimplemented)]
#![deny(clippy::todo)]

use clap::{Parser, Subcommand};
use devices_core::{dispatch, Command};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

#[derive(Parser)]
#[command(
    name = "rust-devices-mcp",
    about = "Pure-Rust devices control: smart plugs + cameras (CLI-first, MCP over stdio)"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show app status (human or --json for agents)
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Retrieve current wattage and energy stats for a smart plug
    PlugStats {
        /// IP address or hostname of the target smart plug
        #[arg(long)]
        device_ip: String,
        #[arg(long)]
        json: bool,
    },
    /// Turn a smart plug on or off (--state true/false)
    PlugToggle {
        #[arg(long)]
        device_ip: String,
        #[arg(long)]
        state: bool,
        #[arg(long)]
        json: bool,
    },
    /// Describe a camera (model, RTSP URL, reachability)
    CameraDescribe {
        #[arg(long)]
        device_ip: String,
        #[arg(long)]
        json: bool,
    },
    /// Snapshot metadata for a camera (dimensions + byte size, no binary blob)
    CameraSnapshot {
        #[arg(long)]
        device_ip: String,
        #[arg(long)]
        json: bool,
    },
    /// Run as MCP server over stdio (MCP 2024-11-05)
    Mcp,
}

fn print_json(value: &Value) {
    if let Ok(text) = serde_json::to_string_pretty(value) {
        println!("{text}");
    } else {
        println!("{{\"error\":\"encode output\"}}");
    }
}

fn print_human(value: &Value) {
    if let Ok(text) = serde_json::to_string_pretty(value) {
        println!("{text}");
    }
}

/// Pure, testable tool catalog. Mirrors `tools/list` without I/O.
fn tools_list() -> Value {
    json!({
        "tools": [
            {
                "name": "status",
                "description": "Show rust-devices-mcp status and command surface",
                "inputSchema": {"type": "object", "properties": {}},
            },
            {
                "name": "plug.get_stats",
                "description": "Retrieve current wattage (watts) and today energy (kWh) for a smart plug",
                "inputSchema": {
                    "type": "object",
                    "properties": {"device_ip": {"type": "string", "description": "IP address or hostname of the target smart plug"}},
                    "required": ["device_ip"],
                },
            },
            {
                "name": "plug.toggle",
                "description": "Turn a smart plug on (state=true) or off (state=false)",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "device_ip": {"type": "string"},
                        "state": {"type": "boolean"},
                    },
                    "required": ["device_ip", "state"],
                },
            },
            {
                "name": "camera.describe",
                "description": "Describe a camera: model, RTSP URL, reachability",
                "inputSchema": {
                    "type": "object",
                    "properties": {"device_ip": {"type": "string"}},
                    "required": ["device_ip"],
                },
            },
            {
                "name": "camera.snapshot_meta",
                "description": "Snapshot metadata (width, height, bytes); no binary blob over MCP",
                "inputSchema": {
                    "type": "object",
                    "properties": {"device_ip": {"type": "string"}},
                    "required": ["device_ip"],
                },
            },
        ]
    })
}

/// Route one `tools/call` by name. Pure dispatch, no I/O.
fn call_tool(name: &str, args: &Value) -> Value {
    let device_ip = args
        .get("device_ip")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let value = match name {
        "status" => dispatch(Command::Status),
        "plug.get_stats" => dispatch(Command::PlugStats { device_ip }),
        "plug.toggle" => {
            let state = args.get("state").and_then(|v| v.as_bool()).unwrap_or(false);
            dispatch(Command::PlugToggle { device_ip, state })
        }
        "camera.describe" => dispatch(Command::CameraDescribe { device_ip }),
        "camera.snapshot_meta" => dispatch(Command::CameraSnapshot { device_ip }),
        _ => json!({"error": format!("unknown tool: {name}")}),
    };
    json!({"content": [{"type": "text", "text": value.to_string()}]})
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Status { json: as_json } => {
            let value = dispatch(Command::Status);
            if as_json {
                print_json(&value);
            } else {
                println!("rust-devices-mcp v0.1.0 - pure Rust, CLI-first");
                println!("commands: status, plug-stats, plug-toggle, camera-describe, camera-snapshot, mcp");
                print_human(&value);
            }
        }
        Commands::PlugStats {
            device_ip,
            json: as_json,
        } => {
            let value = dispatch(Command::PlugStats { device_ip });
            if as_json {
                print_json(&value);
            } else {
                print_human(&value);
            }
        }
        Commands::PlugToggle {
            device_ip,
            state,
            json: as_json,
        } => {
            let value = dispatch(Command::PlugToggle { device_ip, state });
            if as_json {
                print_json(&value);
            } else {
                print_human(&value);
            }
        }
        Commands::CameraDescribe {
            device_ip,
            json: as_json,
        } => {
            let value = dispatch(Command::CameraDescribe { device_ip });
            if as_json {
                print_json(&value);
            } else {
                print_human(&value);
            }
        }
        Commands::CameraSnapshot {
            device_ip,
            json: as_json,
        } => {
            let value = dispatch(Command::CameraSnapshot { device_ip });
            if as_json {
                print_json(&value);
            } else {
                print_human(&value);
            }
        }
        Commands::Mcp => run_mcp_stdio(),
    }
}

/// Minimal MCP 2024-11-05 server over stdio. Same `dispatch` as the CLI, so
/// agents can drive `--json` without MCP or use tools under Claude Desktop.
fn run_mcp_stdio() {
    eprintln!("rust-devices-mcp: MCP server listening on stdio");
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }
        let req: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(e) => {
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": {"code": -32700, "message": format!("parse error: {e}")}
                });
                if let Ok(text) = serde_json::to_string(&resp) {
                    let _ = writeln!(out, "{text}");
                    let _ = out.flush();
                }
                continue;
            }
        };
        let id = req.get("id").cloned().unwrap_or(Value::Null);
        let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let is_notification = req.get("id").is_none();

        let result: Option<Value> = match method {
            "initialize" => Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "rust-devices-mcp", "version": env!("CARGO_PKG_VERSION")},
            })),
            "tools/list" => Some(tools_list()),
            "tools/call" => {
                let params = req.get("params").cloned().unwrap_or(json!({}));
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                Some(call_tool(name, &args))
            }
            "ping" => Some(json!({})),
            _ => {
                if is_notification {
                    None
                } else {
                    let resp = json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {"code": -32601, "message": format!("unknown method: {method}")}
                    });
                    if let Ok(text) = serde_json::to_string(&resp) {
                        let _ = writeln!(out, "{text}");
                        let _ = out.flush();
                    }
                    continue;
                }
            }
        };

        if is_notification {
            continue;
        }
        let body = result.unwrap_or(json!({}));
        let resp = json!({"jsonrpc": "2.0", "id": id, "result": body});
        if let Ok(text) = serde_json::to_string(&resp) {
            let _ = writeln!(out, "{text}");
            let _ = out.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_lists_five_tools() {
        let catalog = tools_list();
        let tools = catalog
            .get("tools")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(tools.len(), 5);
    }

    #[test]
    fn unknown_tool_becomes_error_content_not_panic() {
        let result = call_tool("nope.missing", &json!({}));
        let text = result
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .and_then(|i| i.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("");
        assert!(text.contains("unknown tool"));
    }

    #[test]
    fn toggle_missing_state_defaults_false_without_panic() {
        let result = call_tool("plug.toggle", &json!({"device_ip": "192.168.1.50"}));
        let text = result
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .and_then(|i| i.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("");
        assert!(text.contains("192.168.1.50"));
    }
}
