# Changelog - rust-devices-mcp

## 2026-10-17 - v0.1.0 scaffold (ArtCraft pilot)

- Three-tier Cargo workspace: devices-core (L1), devices-mcp (L2 CLI+MCP), devices-app (L3 Tauri stub).
- Never-crash clippy gates (unwrap/expect/panic/todo/unimplemented deny).
- Command bus: status, plug.get_stats, plug.toggle, camera.describe, camera.snapshot_meta.
- Hand-rolled MCP 2024-11-05 stdio server (stderr logs, pure JSON-RPC stdout).
- justfile: check / test / fmt-check / run-mcp / run-cli / build-release / sync-hourly.
- 2026-10-17: verified green - 13/13 tests, clippy -D warnings clean, fmt clean, stdio MCP probe (init/list/2x call) OK, release bins: rust-devices-mcp.exe 0.81 MB + devices-app.exe 0.19 MB.
