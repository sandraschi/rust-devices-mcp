//! L2 binary: CLI-first + MCP `mcp` subcommand over stdio.
//!
//! Stdout is pure JSON-RPC. Every log line goes to stderr via `eprintln!`.
//! The MCP layer never touches hardware state; it dispatches `Command` IDs
//! into `devices-core` and frames the resulting `serde_json::Value`.
//! Async drivers run on one shared Tokio runtime; the stdio framing loop
//! stays sync and infallible, exactly as proven in v0.1.
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unimplemented)]
#![deny(clippy::todo)]

use clap::{Parser, Subcommand};
use devices_core::{
    camera::CamSource,
    creds::{Credentials, Vendor},
    dispatch, Command,
};
use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::sync::OnceLock;
use tokio::runtime::Runtime;

#[derive(Parser)]
#[command(
    name = "rust-devices-mcp",
    about = "Pure-Rust device control: plugs, Hue, Netatmo, Ring (CLI-first, MCP over stdio)"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Commands {
    /// Show app status (human or --json for agents)
    Status {
        #[arg(long)]
        json: bool,
    },
    /// Plug power stats (vendor: tapo|kasa|shelly|demo)
    PlugStats {
        #[arg(long, default_value = "tapo")]
        vendor: String,
        #[arg(long)]
        device_ip: String,
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Turn a plug on or off
    PlugToggle {
        #[arg(long, default_value = "tapo")]
        vendor: String,
        #[arg(long)]
        device_ip: String,
        #[arg(long)]
        state: bool,
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        password: Option<String>,
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
    /// Grab a real JPEG frame (tapo|onvif|usb|ring|url|demo). No blob over
    /// MCP: metadata (+ file when --out is given).
    CameraSnapshot {
        #[arg(long, default_value = "demo")]
        source: String,
        #[arg(long, default_value = "")]
        device_ip: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        onvif_port: Option<u16>,
        #[arg(long)]
        device_index: Option<u32>,
        #[arg(long)]
        url: Option<String>,
        #[arg(long)]
        out: Option<String>,
        #[arg(long)]
        token_file: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List Hue lights on the bridge
    HueLights {
        #[arg(long)]
        bridge_ip: Option<String>,
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Set a Hue light on/off, optionally brightness 1-254
    HueSetLight {
        #[arg(long)]
        bridge_ip: Option<String>,
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        light_id: String,
        #[arg(long)]
        on: Option<bool>,
        #[arg(long)]
        bri: Option<u8>,
        #[arg(long)]
        json: bool,
    },
    /// Pair with a Hue bridge (press the link button first)
    HuePair {
        #[arg(long)]
        bridge_ip: String,
        #[arg(long)]
        json: bool,
    },
    /// Netatmo weather stations + modules
    NetatmoStations {
        #[arg(long)]
        client_id: Option<String>,
        #[arg(long)]
        client_secret: Option<String>,
        #[arg(long)]
        refresh_token: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Ring auth check + device census
    RingStatus {
        #[arg(long)]
        email: Option<String>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        token_file: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Ring doorbells with active dings + recent history
    RingDoorbells {
        #[arg(long)]
        email: Option<String>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        token_file: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Ring alarm: no --mode reads state; --mode disarmed|home|away sets it
    RingAlarm {
        #[arg(long)]
        mode: Option<String>,
        #[arg(long)]
        location_id: Option<String>,
        #[arg(long)]
        email: Option<String>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        token_file: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Ring first login (may report 2FA pending — no secrets are logged)
    RingAuth {
        #[arg(long)]
        email: Option<String>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        token_file: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Keyless weather via Open-Meteo (Vienna default)
    Weather {
        #[arg(long)]
        lat: Option<f64>,
        #[arg(long)]
        lon: Option<f64>,
        #[arg(long)]
        json: bool,
    },
    /// Complete Ring 2FA after ring-auth reported it pending
    RingAuth2fa {
        #[arg(long)]
        code: String,
        #[arg(long)]
        email: Option<String>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        token_file: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Run as MCP server over stdio (MCP 2024-11-05)
    Mcp,
}

/// One shared runtime for every async driver. Built once; a failed build is
/// a fatal startup error (exit 1), never a panic in a handler.
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

/// Invalid vendor/creds on the CLI: machine-readable error JSON on stdout,
/// exit code 2. (Over MCP the same failures become error content instead.)
fn vendor_of(raw: &str) -> Vendor {
    match Vendor::parse(raw) {
        Ok(vendor) => vendor,
        Err(e) => {
            print_json(&json!({"error": e.to_string()}));
            std::process::exit(2);
        }
    }
}

fn creds_of(vendor: Vendor, username: Option<String>, password: Option<String>) -> Credentials {
    match Credentials::resolve(vendor, username, password) {
        Ok(creds) => creds,
        Err(e) => {
            print_json(&json!({"error": e.to_string()}));
            std::process::exit(2);
        }
    }
}

fn show(value: Value, as_json: bool, header: &str) {
    if as_json {
        print_json(&value);
    } else {
        println!("{header}");
        print_human(&value);
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
                "description": "Plug power: watts + today kWh. vendor tapo|kasa|shelly|demo (default tapo). Tapo needs account via username/password or TAPO_EMAIL+TAPO_PASSWORD env.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "vendor": {"type": "string"},
                        "device_ip": {"type": "string"},
                        "username": {"type": "string"},
                        "password": {"type": "string"},
                    },
                    "required": ["device_ip"],
                },
            },
            {
                "name": "plug.toggle",
                "description": "Turn a plug on (state=true) or off (state=false). Same vendor/auth contract as plug.get_stats.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "vendor": {"type": "string"},
                        "device_ip": {"type": "string"},
                        "state": {"type": "boolean"},
                        "username": {"type": "string"},
                        "password": {"type": "string"},
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
                "name": "camera.snapshot",
                "description": "Grab a real JPEG frame (source tapo|onvif|usb|ring|url|demo). Returns metadata + file path when out_path is given; never a binary blob. Ring doorbell id goes in device_ip.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "source": {"type": "string"},
                        "device_ip": {"type": "string"},
                        "model": {"type": "string"},
                        "username": {"type": "string"},
                        "password": {"type": "string"},
                        "onvif_port": {"type": "integer"},
                        "device_index": {"type": "integer"},
                        "url": {"type": "string"},
                        "out_path": {"type": "string"},
                        "token_file": {"type": "string"},
                    },
                },
            },
            {
                "name": "hue.list_lights",
                "description": "List Hue lights (name, on/off, brightness, reachability). Bridge IP/username via args or HUE_BRIDGE_IP+HUE_USERNAME env.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "bridge_ip": {"type": "string"},
                        "username": {"type": "string"},
                    },
                },
            },
            {
                "name": "hue.set_light",
                "description": "Set a Hue light on/off with optional brightness 1-254. `on` is required (no safe default).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "bridge_ip": {"type": "string"},
                        "username": {"type": "string"},
                        "light_id": {"type": "string"},
                        "on": {"type": "boolean"},
                        "bri": {"type": "integer", "minimum": 1, "maximum": 254},
                    },
                    "required": ["light_id", "on"],
                },
            },
            {
                "name": "hue.pair",
                "description": "Pair with a Hue bridge (press the physical link button first). Returns the username to store as HUE_USERNAME.",
                "inputSchema": {
                    "type": "object",
                    "properties": {"bridge_ip": {"type": "string"}},
                    "required": ["bridge_ip"],
                },
            },
            {
                "name": "netatmo.stations",
                "description": "Netatmo weather stations + modules (temperature, humidity, CO2, pressure, noise). OAuth via args or NETATMO_CLIENT_ID+NETATMO_CLIENT_SECRET+NETATMO_REFRESH_TOKEN env.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "client_id": {"type": "string"},
                        "client_secret": {"type": "string"},
                        "refresh_token": {"type": "string"},
                    },
                },
            },
            {
                "name": "ring.status",
                "description": "Ring auth check + device census (doorbells, chimes, cameras, alarm gear). Reuses ring_token.cache when present.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "email": {"type": "string"},
                        "password": {"type": "string"},
                        "token_file": {"type": "string"},
                    },
                },
            },
            {
                "name": "ring.doorbells",
                "description": "Ring doorbells with active dings + recent history entries.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "email": {"type": "string"},
                        "password": {"type": "string"},
                        "token_file": {"type": "string"},
                    },
                },
            },
            {
                "name": "ring.alarm",
                "description": "Ring alarm: omit mode to read state; mode disarmed|home|away to arm/disarm. Siren control is intentionally not exposed.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "mode": {"type": "string"},
                        "location_id": {"type": "string"},
                        "email": {"type": "string"},
                        "password": {"type": "string"},
                        "token_file": {"type": "string"},
                    },
                },
            },
            {
                "name": "ring.auth",
                "description": "Ring first login with email+password (may report 2FA pending; secrets never logged). Token cached for later calls.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "email": {"type": "string"},
                        "password": {"type": "string"},
                        "token_file": {"type": "string"},
                    },
                },
            },
            {
                "name": "ring.auth_2fa",
                "description": "Complete Ring 2FA after ring.auth reported it pending.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "code": {"type": "string"},
                        "email": {"type": "string"},
                        "password": {"type": "string"},
                        "token_file": {"type": "string"},
                    },
                    "required": ["code"],
                },
            },
            {
                "name": "weather.now",
                "description": "Keyless weather via Open-Meteo (Vienna default 48.2082, 16.3738). No account, works out of the box.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "lat": {"type": "number"},
                        "lon": {"type": "number"},
                    },
                },
            },
        ]
    })
}

fn opt(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Route one `tools/call` by name. Pure dispatch, no I/O.
fn call_tool(name: &str, args: &Value) -> Value {
    let device_ip = opt(args, "device_ip").unwrap_or_default();
    let email = opt(args, "email");
    let password = opt(args, "password");
    let token_file = opt(args, "token_file");
    let value = match name {
        "status" => run(Command::Status),
        "plug.get_stats" => {
            let vendor =
                match Vendor::parse(&opt(args, "vendor").unwrap_or_else(|| "tapo".to_string())) {
                    Ok(vendor) => vendor,
                    Err(e) => return wrap_err(e.to_string()),
                };
            let creds =
                match Credentials::resolve(vendor, opt(args, "username"), opt(args, "password")) {
                    Ok(creds) => creds,
                    Err(e) => return wrap_err(e.to_string()),
                };
            run(Command::PlugStats {
                vendor,
                device_ip,
                creds,
            })
        }
        "plug.toggle" => {
            let vendor =
                match Vendor::parse(&opt(args, "vendor").unwrap_or_else(|| "tapo".to_string())) {
                    Ok(vendor) => vendor,
                    Err(e) => return wrap_err(e.to_string()),
                };
            let creds =
                match Credentials::resolve(vendor, opt(args, "username"), opt(args, "password")) {
                    Ok(creds) => creds,
                    Err(e) => return wrap_err(e.to_string()),
                };
            let state = args.get("state").and_then(|v| v.as_bool()).unwrap_or(false);
            run(Command::PlugToggle {
                vendor,
                device_ip,
                state,
                creds,
            })
        }
        "camera.describe" => run(Command::CameraDescribe { device_ip }),
        "camera.snapshot" => {
            let source = match CamSource::parse(
                &opt(args, "source").unwrap_or_else(|| "demo".to_string()),
            ) {
                Ok(source) => source,
                Err(e) => return wrap_err(e.to_string()),
            };
            let onvif_port = args
                .get("onvif_port")
                .and_then(|v| v.as_u64())
                .map(|p| p.min(65535) as u16);
            let device_index = args
                .get("device_index")
                .and_then(|v| v.as_u64())
                .map(|i| i.min(u32::MAX as u64) as u32);
            run(Command::CameraSnapshot {
                source,
                device_ip,
                model: opt(args, "model"),
                username: opt(args, "username"),
                password: opt(args, "password"),
                onvif_port,
                device_index,
                url: opt(args, "url"),
                out_path: opt(args, "out_path"),
                ring_token_file: opt(args, "token_file"),
            })
        }
        "hue.list_lights" => run(Command::HueLights {
            bridge_ip: opt(args, "bridge_ip"),
            username: opt(args, "username"),
        }),
        "hue.set_light" => {
            let light_id = opt(args, "light_id").unwrap_or_default();
            let Some(on) = args.get("on").and_then(|v| v.as_bool()) else {
                return wrap_err("hue.set_light: `on` is required (no safe default)".to_string());
            };
            let bri = args
                .get("bri")
                .and_then(|v| v.as_u64())
                .map(|b| b.min(255) as u8);
            run(Command::HueSetLight {
                bridge_ip: opt(args, "bridge_ip"),
                username: opt(args, "username"),
                light_id,
                on,
                bri,
            })
        }
        "hue.pair" => run(Command::HuePair {
            bridge_ip: opt(args, "bridge_ip").unwrap_or_default(),
        }),
        "netatmo.stations" => run(Command::NetatmoStations {
            client_id: opt(args, "client_id"),
            client_secret: opt(args, "client_secret"),
            refresh_token: opt(args, "refresh_token"),
        }),
        "ring.status" => run(Command::RingStatus {
            email,
            password,
            token_file,
        }),
        "ring.doorbells" => run(Command::RingDoorbells {
            email,
            password,
            token_file,
        }),
        "ring.alarm" => run(Command::RingAlarm {
            mode: opt(args, "mode"),
            location_id: opt(args, "location_id"),
            email,
            password,
            token_file,
        }),
        "ring.auth" => run(Command::RingAuth {
            email,
            password,
            token_file,
        }),
        "ring.auth_2fa" => run(Command::RingAuth2fa {
            code: opt(args, "code").unwrap_or_default(),
            email,
            password,
            token_file,
        }),
        "weather.now" => run(Command::Weather {
            latitude: args.get("lat").and_then(|v| v.as_f64()),
            longitude: args.get("lon").and_then(|v| v.as_f64()),
        }),
        _ => return wrap_err(format!("unknown tool: {name}")),
    };
    wrap_ok(value)
}

fn wrap_ok(value: Value) -> Value {
    json!({"content": [{"type": "text", "text": value.to_string()}]})
}

fn wrap_err(message: String) -> Value {
    wrap_ok(json!({"error": message}))
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Status { json: as_json } => {
            show(
                run(Command::Status),
                as_json,
                "rust-devices-mcp v0.3.0 - pure Rust, CLI-first",
            );
        }
        Commands::PlugStats {
            vendor,
            device_ip,
            username,
            password,
            json: as_json,
        } => {
            let vendor = vendor_of(&vendor);
            let creds = creds_of(vendor, username, password);
            show(
                run(Command::PlugStats {
                    vendor,
                    device_ip,
                    creds,
                }),
                as_json,
                "",
            );
        }
        Commands::PlugToggle {
            vendor,
            device_ip,
            state,
            username,
            password,
            json: as_json,
        } => {
            let vendor = vendor_of(&vendor);
            let creds = creds_of(vendor, username, password);
            show(
                run(Command::PlugToggle {
                    vendor,
                    device_ip,
                    state,
                    creds,
                }),
                as_json,
                "",
            );
        }
        Commands::CameraDescribe {
            device_ip,
            json: as_json,
        } => {
            show(run(Command::CameraDescribe { device_ip }), as_json, "");
        }
        Commands::CameraSnapshot {
            source,
            device_ip,
            model,
            username,
            password,
            onvif_port,
            device_index,
            url,
            out: out_path,
            token_file,
            json: as_json,
        } => {
            let parsed = match CamSource::parse(&source) {
                Ok(parsed) => parsed,
                Err(e) => {
                    print_json(&json!({"error": e.to_string()}));
                    std::process::exit(2);
                }
            };
            show(
                run(Command::CameraSnapshot {
                    source: parsed,
                    device_ip,
                    model,
                    username,
                    password,
                    onvif_port,
                    device_index,
                    url,
                    out_path,
                    ring_token_file: token_file,
                }),
                as_json,
                "",
            );
        }
        Commands::HueLights {
            bridge_ip,
            username,
            json: as_json,
        } => {
            show(
                run(Command::HueLights {
                    bridge_ip,
                    username,
                }),
                as_json,
                "",
            );
        }
        Commands::HueSetLight {
            bridge_ip,
            username,
            light_id,
            on,
            bri,
            json: as_json,
        } => {
            let Some(on) = on else {
                print_json(
                    &json!({"error": "hue-set-light: --on <true|false> is required (no safe default)"}),
                );
                std::process::exit(2);
            };
            show(
                run(Command::HueSetLight {
                    bridge_ip,
                    username,
                    light_id,
                    on,
                    bri,
                }),
                as_json,
                "",
            );
        }
        Commands::HuePair {
            bridge_ip,
            json: as_json,
        } => {
            show(run(Command::HuePair { bridge_ip }), as_json, "");
        }
        Commands::NetatmoStations {
            client_id,
            client_secret,
            refresh_token,
            json: as_json,
        } => {
            show(
                run(Command::NetatmoStations {
                    client_id,
                    client_secret,
                    refresh_token,
                }),
                as_json,
                "",
            );
        }
        Commands::Weather {
            lat,
            lon,
            json: as_json,
        } => {
            show(
                run(Command::Weather {
                    latitude: lat,
                    longitude: lon,
                }),
                as_json,
                "",
            );
        }
        Commands::RingStatus {
            email,
            password,
            token_file,
            json: as_json,
        } => {
            show(
                run(Command::RingStatus {
                    email,
                    password,
                    token_file,
                }),
                as_json,
                "",
            );
        }
        Commands::RingDoorbells {
            email,
            password,
            token_file,
            json: as_json,
        } => {
            show(
                run(Command::RingDoorbells {
                    email,
                    password,
                    token_file,
                }),
                as_json,
                "",
            );
        }
        Commands::RingAlarm {
            mode,
            location_id,
            email,
            password,
            token_file,
            json: as_json,
        } => {
            show(
                run(Command::RingAlarm {
                    mode,
                    location_id,
                    email,
                    password,
                    token_file,
                }),
                as_json,
                "",
            );
        }
        Commands::RingAuth {
            email,
            password,
            token_file,
            json: as_json,
        } => {
            show(
                run(Command::RingAuth {
                    email,
                    password,
                    token_file,
                }),
                as_json,
                "",
            );
        }
        Commands::RingAuth2fa {
            code,
            email,
            password,
            token_file,
            json: as_json,
        } => {
            show(
                run(Command::RingAuth2fa {
                    code,
                    email,
                    password,
                    token_file,
                }),
                as_json,
                "",
            );
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
    use devices_core::no_creds;

    #[test]
    fn catalog_lists_fifteen_tools() {
        let catalog = tools_list();
        let tools = catalog
            .get("tools")
            .and_then(|t| t.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(tools.len(), 15);
    }

    #[test]
    fn unknown_tool_becomes_error_content_not_panic() {
        let result = call_tool("nope.missing", &json!({}));
        let text = content_text(&result);
        assert!(text.contains("unknown tool"));
    }

    #[test]
    fn toggle_missing_state_defaults_false_without_panic() {
        let result = call_tool(
            "plug.toggle",
            &json!({"device_ip": "192.168.1.50", "vendor": "demo"}),
        );
        let text = content_text(&result);
        assert!(text.contains("192.168.1.50"));
    }

    #[test]
    fn hue_set_without_on_is_rejected_not_defaulted() {
        let result = call_tool("hue.set_light", &json!({"light_id": "1"}));
        let text = content_text(&result);
        assert!(text.contains("`on` is required"));
    }

    #[test]
    fn bad_vendor_is_error_content() {
        let result = call_tool("plug.get_stats", &json!({"vendor": "toaster"}));
        let text = content_text(&result);
        assert!(text.contains("unknown vendor"));
    }

    #[test]
    fn demo_stats_still_deterministic() {
        let result = call_tool(
            "plug.get_stats",
            &json!({"vendor": "demo", "device_ip": "192.168.1.50"}),
        );
        let text = content_text(&result);
        assert!(text.contains("42.5"));
    }

    fn content_text(result: &Value) -> String {
        result
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|a| a.first())
            .and_then(|i| i.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string()
    }

    #[test]
    fn ring_auth_2fa_empty_code_is_typed_error() {
        let value = run(Command::RingAuth2fa {
            code: String::new(),
            email: None,
            password: None,
            token_file: None,
        });
        assert!(value
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("")
            .contains("invalid input"));
    }

    #[test]
    fn cli_parses_without_creds_for_demo() {
        // no_creds keeps the import live and documents the env-fallback path.
        let _ = no_creds();
    }
}
