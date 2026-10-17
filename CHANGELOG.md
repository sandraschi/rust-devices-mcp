# Changelog - rust-devices-mcp

> Project status: **ALPHA** (2026-10-17). What's live vs. stub is tracked in
> README's verification table and docs/PRD.md — this log records changes only.

## v0.4.0 (2026-10-17) - live video, bridge, help, import fixes

- Crossconnect: `bridge.rs` client (aiwatcher events, speech-mcp TTS,
  generic webhooks) + `bridge.event/speak/webhook` tools (18 total) +
  Crossconnect page with call log. Speech proven live (server spoke).
  Verdict: no new bridge server needed.
- Help page: first-run guide, credentials cheat sheet, UI tricks, links.
- Live RTSP video (no ffmpeg) below (unchanged).

### Live RTSP video (no ffmpeg) — same release

- `video.rs`: retina RTSP session (describe/setup/play/demux) + openh264
  decode + Annex-B framing converter, feeding RGB frames to the UI player.
  Proven live: 3 decoded frames in 0.46s from a Tapo camera (orchard: the
  first attempt failed on retina's "URL must not contain credentials" —
  login travels via session options, and ONVIF URIs get stripped).
- UI: Live/Stop player on the Cameras page (RTSP pump for tapo/onvif
  streams, ~0.5 fps snapshot poller for ring/usb/url/demo), FPS counter,
  texture reuse via `TextureHandle::set`.
- openh264 builds sans-NASM on MSVC (bundled source; NASM only accelerates).

## v0.3.0 (2026-10-17) - fleet import, NSIS, UI polish, live proofs

- Fleet import (`import.rs`, operator-permitted): plugs, Hue, ONVIF/tapo
  cameras with per-camera logins, Ring token path, Netatmo triple, USB index
  from the devices-mcp checkout; readonly-plug toggle guard; per-device env
  names (`TAPO_P115_*`, `TAPO_LIGHTING_*`, `TAPO_CAMERA_2_*`,
  `HUE_BRIDGE_USERNAME`); live-oracle tests against the real config.
- Live proofs: Tapo plugs (15.7 W / 290 W — caught an mW→W unit bug),
  Tapo cam 640x360, Netatmo station, Hue 18 lights, Ring doorbell+snapshot,
  USB 640x480. ONVIF-generic documented quirk (Tapo faults GetSnapshotUri).
- UI polish per fleet canon: Ctrl+wheel zoom + pill (persisted), light/dark
  switcher with vector sun/moon, hairlines + outlines, accent stripes,
  zero-dep GPU spark charts (power history, 24h temperature via new
  Open-Meteo hourly payload), self-scanning glyph guard.
- Hand-written NSIS installer (current-user, Start Menu, uninstaller).
- UI parity flow doc, PRD, alpha marking, kittest goldens (3).

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
- Fleet import (`import.rs`, operator-permitted): IPs, per-camera logins,
  Hue/Ring/Netatmo auth from the devices-mcp checkout; readonly-plug toggle
  guard; per-device Tapo env names (`TAPO_P115_*`, `TAPO_LIGHTING_*`,
  `TAPO_CAMERA_2_*`, `HUE_BRIDGE_USERNAME`).
- Live-hardware fixes: Tapo energy `current_power` is milliwatts (crate docs
  confirm; 290 kW readings were the tell), Ring refresh needs minimal body +
  User-Agent, Tapo ONVIF faults GetSnapshotUri (native path used).
- Hand-written NSIS installer (current-user, no admin): Start Menu +
  Add/Remove Programs entries, ~17 MB bundle.
- Fixes from live use: `${VAR:-default}` expansion in fleet import (config
  hosts arrived unexpanded and prefills broke — regression-tested), visible
  dark-mode card borders (black shadows are physically invisible; borders
  carry separation), curated weather card (hourly arrays flooded the old
  one), spinning GPU icosahedron on the dashboard (12 verts, 20 faces, zero
  new crates, projection unit-tested), ONVIF multi-profile snapshot attempts
  (Tapo firmware faults all profiles — documented quirk, native path used).
- UI parity flow (`docs/UI_PARITY.md`): oracle screenshots → egui clone →
  kittest golden snapshots + glyph-allowlist guard (egui 0.37 bundles only
  Ubuntu-Light + egui-icons; status dots are painted, never glyphs).
- Fleet-standard polish: Ctrl+wheel zoom + zoom pill (persisted), light/dark
  theme switcher with vector sun/moon, hairline separators, button outlines,
  accent stripes on KPI cards, zero-dep GPU spark charts (power history +
  24h temperature curve via new Open-Meteo hourly payload), self-scanning
  glyph guard (`include_str!`) stronger than the render guard.

## 2026-10-17 - v0.1.0 scaffold (ArtCraft pilot)

- Three-tier Cargo workspace: devices-core (L1), devices-mcp (L2 CLI+MCP), devices-app (L3 Tauri stub).
- Never-crash clippy gates (unwrap/expect/panic/todo/unimplemented deny).
- Command bus: status, plug.get_stats, plug.toggle, camera.describe, camera.snapshot_meta.
- Hand-rolled MCP 2024-11-05 stdio server (stderr logs, pure JSON-RPC stdout).
- justfile: check / test / fmt-check / run-mcp / run-cli / build-release / sync-hourly.
- 2026-10-17: verified green - 13/13 tests, clippy -D warnings clean, fmt clean, stdio MCP probe (init/list/2x call) OK, release bins: rust-devices-mcp.exe 0.81 MB + devices-app.exe 0.19 MB.
