# rust-devices-mcp

Pure-Rust conversion template for `devices-mcp` (smart plug power monitoring,
camera controls, local IoT bridge). ArtCraft-methodology pilot: three-tier Cargo
workspace, never-crash lints, CLI-first + MCP subcommand in one binary.

## Why this repo exists

`devices-mcp` (Python/FastMCP) bundles a ~150 MB interpreter, slow cold boot
(2-4 s), and venv/PyInstaller fragility. This repo proves the Rust-only target:

- Single `cargo build --release` produces self-contained binaries (~8-15 MB installer).
- Cold boot target < 50 ms (no interpreter import loop).
- Same `devices-core` crate drives the CLI, the MCP server (stdio), and the
  Tauri desktop shell in-process (no local HTTP hop).

## Workspace layout

```text
rust-devices-mcp/
  Cargo.toml                  # virtual workspace + clippy deny gates
  justfile                    # check / test / run-mcp / build-release / sync-hourly
  AGENTS.md                   # ArtCraft containment harness (read before coding)
  crates/
    devices-core/             # L1: protocols, state machine, command bus (no UI/MCP deps)
    devices-mcp/              # L2: CLI-first binary, `mcp` subcommand over stdio
    devices-app/              # L3: Tauri v2 shell calling devices-core in-process
```

## Quickstart (Windows PowerShell 7)

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo run -p rust-devices-mcp -- status
cargo run -p rust-devices-mcp -- plug-stats --device-ip 192.168.1.50 --json
cargo run -p rust-devices-mcp -- mcp   # MCP 2024-11-05 server over stdio
```

## MCP contract (stdio, JSON-RPC 2024-11-05)

Tools: `status`, `plug.get_stats`, `plug.toggle`, `camera.describe`,
`camera.snapshot_meta`. Logs on stderr, frames on stdout. Notifications get no reply.

Minimal probe (PowerShell):

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
'{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | cargo run -q -p rust-devices-mcp -- mcp
```

## Conversion scope (from devices-mcp Python)

Ported first (highest value, strictest I/O): plug power stats/toggle
(`tools/energy`, Tapo P115 via `tapo`/`python-kasa`), camera describe/snapshot
(`tools/camera`, `server_v2.py` transport pattern, stderr-logging rule).

Deferred to later scorecard tiers: vision/OpenCV, voice/STT/TTS, Plex, ONVIF,
Ring/Nest/Hue bridges, SDR, web dashboard. Each deferred tier becomes one
`Command` variant + dispatch test when promoted.

## rmcp note (deliberate v0.1 deviation)

The planning brief specified `rmcp 1.8` with derive macros. v0.1 ships the
`rust-test-b`-proven hand-rolled stdio framing instead (zero new deps, builds
offline, byte-exact stdout control). The tool schemas and dispatch surface are
kept `rmcp`-shaped so migration is mechanical: replace `run_mcp_stdio` with
`#[tool_router]` impls calling the same `devices_core::dispatch`. Do not adopt
`rmcp` until its version is pinned and audited against the stderr invariant.

## Definition of done

See `AGENTS.md` section 4. `just check`, `just test`, stdio smoke, CHANGELOG line.
