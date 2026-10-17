# PRD — rust-devices-mcp (Product Requirements Document)

Status: **ALPHA** (2026-10-17). Living document — update when tiers land.

## 1. Vision

Prove the fleet can leave Python: one pure-Rust workspace that controls
Sandra's real smart home (plugs, lights, weather, cameras, doorbell, alarm)
through three faces sharing one command bus — CLI (`--json`), MCP stdio
(15 tools), and a native egui dashboard. No interpreter, no venv, no
WebView2, ~31 MB self-contained desktop app + ~11 MB CLI server.

Success looks like: the Python `devices-mcp` can be stopped and nothing in
the house stops working.

## 2. Users

Sandra (primary, technical, Vienna LAN). Agents via MCP (secondary).
No general-public release planned in alpha — binaries are unsigned.

## 3. Functional requirements

| # | Requirement | State (2026-10-17) |
|---|---|---|
| F1 | Tapo plugs: stats (W + today kWh) + toggle (P100/P105/P110/P115) | ✅ Live (.17, .38 real readings) |
| F2 | Kasa plugs: stats + toggle (XOR → KLAP auto) | ⏳ Code-complete, no hardware on LAN |
| F3 | Shelly plugs: stats + toggle (Gen1/Gen2 probe) | ⏳ Code-complete, no hardware on LAN |
| F4 | Hue: list/set lights + link-button pairing | ✅ Live (18 lights) |
| F5 | Weather, keyless (Open-Meteo, Vienna default) | ✅ Live |
| F6 | Netatmo stations + modules | ✅ Live (Stroheckgasse) |
| F7 | Ring: auth/2FA, status, doorbells + history, snapshots | ✅ Live (Front Door) |
| F8 | Ring alarm: read state, arm/disarm | ⏳ Code-complete, no alarm hardware on account; siren NEVER (safety) |
| F9 | Camera snapshots: tapo / onvif / usb / ring / url → file + metadata | ✅ Live (tapo, usb, ring, url, demo; ONVIF-generic pending non-Tapo HW) |
| F10 | Fleet import: IPs + auth from devices-mcp checkout (permitted) | ✅ Live (readonly-plug guard included) |
| F11 | Current-user NSIS installer + uninstaller | ✅ Live (16.8 MB bundle) |
| F12 | MCP stdio server, 15 tools, stderr-logging invariant | ✅ Live (18 tools since bridge) |
| F13 | `--smoke` headless parity path for CI | ✅ Live |
| F14 | Live RTSP video in-app (retina + openh264, FPS player) | ✅ Live (Tapo cam, 3 frames/0.46s oracle) |
| F15 | Crossconnect client (aiwatcher/speech/webhook, no new server) | ✅ Live (server spoke; aiwatcher-down path graceful) |
| F16 | Help page + parity flow + glyph guards | ✅ Live |

## 4. UI/UX requirements

- Sidebar navigation mirroring the webapp (Dashboard, Energy, Lighting,
  Climate, Cameras, Weather, Alarms, Raw), retractable, per-device pages
  for Hue lights and Ring doorbells.
- High-contrast theme (near-white on slate-950, 16px base), KPI hero row,
  card/pill primitives. Glyph rule: bundled fonts cover ASCII + `°×·…—`
  only — status dots are painted, never glyphs (guarded by
  `glyph_allowlist` test).
- Fields persist across restarts; **passwords never persist**.
- First paint is never empty (status + weather + whatever env provides).
- Beauty parity with the webapp follows `docs/UI_PARITY.md`
  (oracle screenshots → clone → kittest goldens). Sandra judges taste;
  machines guard regressions.

## 5. Security requirements

- Secrets: explicit args > process env > devices-mcp `.env` > config;
  never logged, never in errors, never committed (no `.env` in repo,
  `*.cache` gitignored). Ring token refresh writes only the operator's own
  cache path.
- No destructive actuation without intent: readonly plugs refuse toggles,
  `hue.set_light` requires explicit `on`, siren endpoint not implemented.
- Unsigned binaries in alpha (SmartScreen warning expected); signing is a
  release-tier decision, not a code change.

## 6. Non-goals (alpha)

Tuya/Meross/Gosund (absent from the Python repo — nothing to port), Wien
Energie serial meters, iKettle, Nest, Midea/Mitsubishi climate pages (UI
shows honest placeholders), Kasa/Shelly live proof (no hardware), signed
installer, general-public support.

## 7. Acceptance criteria per tier (done)

- Tier-2/3: real driver + live probe on Sandra's LAN (or documented blocker
  + typed-degradation proof) + unit tests + CHANGELOG line.
- Tier-4: window opens, all tabs render with live data where creds exist,
  `--smoke` green, release assets (exes + installer + checksums) uploaded.
- Gates, always: `cargo test`, `clippy -D warnings`, `fmt --check` green.

## 8. Roadmap

v0.3: climate drivers + pages, Kasa/Shelly if hardware appears, signed
installer decision, MCPB/second-distribution packaging per fleet norms.
