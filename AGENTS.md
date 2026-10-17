# AGENTS.md - rust-devices-mcp (ArtCraft containment harness, adapted)

Pure-Rust conversion template for `devices-mcp` (smart plug power monitoring,
camera controls, local IoT bridge). Proves the Rust-only path before fleet rollout.

## 1. Safety and error handling (never crash)

- Workspace lint gate (enforced in root `Cargo.toml`):
  `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`,
  `clippy::unimplemented`, `clippy::todo` are all `deny`.
- Every fallible function returns `Result<T, DeviceError>` and bubbles with `?`.
  Use `ok_or_else` / explicit `match`, never `unwrap` / `expect`.
- Treat all external input as hostile (CLI args, MCP payloads, socket bytes):
  - No direct indexing (`buf[i]`); use `.get()` with a typed error on `None`.
  - Integer math on lengths / rates / wattage uses `checked_*` / `saturating_*`.
  - String slicing only at `char` boundaries (`str::get`, never byte slicing).
  - Validate `device_ip` non-empty before any socket touch.
- Lock poisoning must never cascade: `lock().unwrap_or_else(PoisonError::into_inner)`
  pattern or restructure so workers report error events instead of killing the server.
- Every bug fix MUST ship a minimal regression test that fails before the fix.
- Stdio MCP invariant: **stdout is pure JSON-RPC only**. All logs go to stderr
  via `eprintln!`. Audit every crate for rogue `println!` outside the framed writer.

## 2. Architecture and seams

- L1 `devices-core`: device protocols, state machine, command bus. Zero UI,
  zero MCP, zero Tauri dependencies. Compiles with only `serde` + `serde_json`.
- L2 `devices-mcp`: CLI-first binary + MCP `mcp` subcommand over stdio
  (MCP 2024-11-05, hand-rolled framing proven in `rust-test-b`). No `rmcp` dep
  in v0.1 (offline-build guarantee); `rmcp` migration is documented, not attempted
  blindly. The MCP layer only dispatches `Command` IDs into `devices-core`.
- L3 `devices-app`: Tauri v2 shell. Calls `devices-core` functions directly
  in-process via `#[tauri::command]` handlers. Never shells out to the MCP binary,
  never uses local HTTP/JSON-RPC for the desktop path.
- No agent may invent ad-hoc state transitions between UI widgets and core buffers.
  New capability = new `Command` variant + typed params + undo checkpoint note +
  dispatch-loop unit test.

## 3. MCP surface contract

- Every command exposed via MCP MUST have:
  1. A unique string command identifier (`plug.get_stats`, `plug.toggle`,
     `camera.describe`, `camera.snapshot_meta`, `status`).
  2. Strongly typed deserializable params with JSON Schema in `tools/list`.
  3. A headless dispatch unit test (no hardware, no network).
- `tools/list` descriptions are the agent contract: keep them short, imperative,
  with units (watts, kWh) and arg ranges.
- Notifications (`notifications/initialized`) get no response. Unknown methods
  return JSON-RPC `-32601`, bad params `-32602`, parse failures `-32700`.

## 4. Definition of done

- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` green.
- [ ] `cargo test --workspace` green (incl. new regression test for any fix).
- [ ] `cargo fmt --all --check` green.
- [ ] Round-trip serialization test for any new schema (params or `PowerStats`).
- [ ] `tools/list` + `tools/call` smoke-tested over stdio with byte-exact framing.
- [ ] CHANGELOG.md checkpoint line added.

## 5. Fleet notes

- Windows-first: all `just` recipes use `windows-shell` + explicit
  `$env:USERPROFILE\.cargo\bin` prefix (cargo is not on inherited PATH).
- Never hardcode ports, IPs, or machine paths in runtime code. Test fixtures may
  use `192.168.1.50` as documentation only; production IP always comes from args.
- Source of truth for conversion scope: `D:\Dev\repos\devices-mcp\src\devices_mcp`
  (`tools/`, `server_v2.py`, `transport.py`). Port the plug/camera command surface
  first; leave vision/voice/plex/ONVIF for later scorecard tiers.
