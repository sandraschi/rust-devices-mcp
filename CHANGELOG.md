# Changelog - rust-devices-mcp

## v0.2.0 (2026-10-17) - real drivers (tier-2 + tier-3) + egui dashboard + keyless weather

- Plugs for ALL vendors: Tapo P100/P105/P110/P115 (`tapo` crate, P115→P110
  fallback), Kasa legacy XOR + KLAP auto (`kasa-rs`, incl. UDP-shape JSON ops),
  Shelly Gen1/Gen2/Gen3 (`shelly-core` probe + switch/power). `--vendor`
  flag everywhere; `demo` vendor keeps the deterministic oracle.
- Hue lighting: CLIP v1 list/set + link-button `pair` (hand-rolled, no Hue
  crate made the cut). Proven live: 18 lights on 192.168.0.236.
- Netatmo: OAuth2 refresh + stations/modules summary (hand-rolled; awaits secret).
- Ring: password grant + 2FA flow + refresh-token reuse + doorbells/dings/
  history + alarm read/arm/disarm + doorbell JPEG snapshots. Proven live:
  Front Door auth, motion history, 640x360 snapshot. Siren deliberately absent.
- Cameras: real frames from tapo/onvif/usb/ring/url; std-only SOF dimension
  parser (incl. APP0-skip regression test from a live frame); `--out` files,
  metadata-only MCP. USB (640x480) + URL + Ring proven live.
- 14 MCP tools;typed Auth fence naming exact env vars; `*.cache` gitignored.
- Live-hardware bug bounty so far: JPEG segment-length off-by-2, Ring minimal
  refresh body, Ring User-Agent 406 — each with a regression test or comment.
- 28/28 tests green, clippy `-D warnings` clean, fmt clean.
- egui dashboard: sidebar navigation mirroring the webapp (Dashboard, Energy,
  Lighting + per-light pages, Climate placeholder, Cameras, Weather, Alarms +
  per-doorbell pages, Raw), high-contrast theme + 16px base, field persistence
  (never passwords), `--smoke` headless CI. Tauri shell prototyped (4.64 MiB
  NSIS) then removed for the standard-mandated native stack.

## 2026-10-17 - v0.1.0 scaffold (ArtCraft pilot)

- Three-tier Cargo workspace: devices-core (L1), devices-mcp (L2 CLI+MCP), devices-app (L3 Tauri stub).
- Never-crash clippy gates (unwrap/expect/panic/todo/unimplemented deny).
- Command bus: status, plug.get_stats, plug.toggle, camera.describe, camera.snapshot_meta.
- Hand-rolled MCP 2024-11-05 stdio server (stderr logs, pure JSON-RPC stdout).
- justfile: check / test / fmt-check / run-mcp / run-cli / build-release / sync-hourly.
- 2026-10-17: verified green - 13/13 tests, clippy -D warnings clean, fmt clean, stdio MCP probe (init/list/2x call) OK, release bins: rust-devices-mcp.exe 0.81 MB + devices-app.exe 0.19 MB.
