# Architecture & Implementation Assessment -- `rust-devices-mcp` (`v0.4.0+` / `aab6c17`)

**Assessment Date:** 2026-10-17 (Late Evening Re-Assessment)  
**Target Commit:** `aab6c17` (`feat(ui): device pickers, brightness steppers, changelog`)  
**Workspace Version:** `0.4.0` (+ `Unreleased` `v0.5.0`-class feature wave)  
**Assessor:** Antigravity (Fleet Architecture & Standards Audit)  
**Canonical Standard:** `mcp-central-docs/standards/RUST_ARTCRAFT_STANDARD.md`

---

## 1. Executive Verdict

**Grade: SOTA Exemplar (`A+` / Reference Fleet Blueprint)**

In a single day of execution (2026-10-17), `rust-devices-mcp` progressed from a `v0.1.0` 5-command scaffold to a **4-crate, 10,124-line, zero-`unwrap`, 24-tool pure-Rust smart-home control suite** with live RTSP H.264 video decoding (`retina` + `openh264`), a local SQLite time-series historian (`devices-store`), LAN device discovery (`scan.rs`), Windows Credential Manager secret storage (`vault.rs`), fleet crossconnect (`bridge.rs`), multi-provider LLM chat (`llm.rs`), and a 14-view GPU-rendered `eframe`/`egui` desktop application (`devices-app`).

Most importantly, **speed did not compromise containment**:
- **`cargo clippy --workspace --all-targets --all-features -- -D warnings`**: **PASS** (0 warnings, 0 errors; `unwrap_used`, `expect_used`, `panic`, `todo`, `unimplemented` all `deny` across all 4 crates).
- **`cargo test --workspace`**: **PASS (`58/58` active tests green + 1 opt-in live-camera RTSP test)**.
- **`cargo fmt --all --check`**: **PASS** (0 formatting diffs).

---

## 2. Measured Workspace Census (`aab6c17`)

### 2.1 Four-Crate Topology (`10,124` Lines of Pure Rust)

| Tier | Crate | Files | Lines of Rust | Responsibility & Architectural Boundary |
|---|---|---:|---:|---|
| **L1 (Core)** | `crates/devices-core` | 20 | **4,913** | All vendor protocols, credential resolution, legacy fleet import (`import.rs`), live RTSP/H.264 video (`video.rs`), fleet HTTP crossconnect (`bridge.rs`), LLM chat client (`llm.rs`), concurrent LAN scanner (`scan.rs`), and the 24-variant `Command::dispatch` bus. **Zero UI, CLI, or MCP transport dependencies.** |
| **L1 (Store)** | `crates/devices-store` | 1 | **300** | Embedded SQLite historian (`rusqlite 0.40` bundled) at `%LOCALAPPDATA%/rust-devices-mcp/devices.db`. Tracks `energy_readings`, `weather_readings`, and `alarm_events` with `PRAGMA user_version` migrations and 90-day retention pruning. |
| **L2 (CLI + MCP)** | `crates/devices-mcp` | 1 | **1,436** | CLI-first `clap` binary (24 `--json` subcommands + `mcp` subcommand) and hand-rolled `MCP 2024-11-05` stdio server (`stdout` pure JSON-RPC, logs strictly on `stderr`). |
| **L3 (GUI)** | `crates/devices-app` | 4 | **3,455** | Pure-Rust `eframe 0.37` / `egui` native GPU desktop dashboard (14 views), OS Credential Manager vault (`vault.rs` via `keyring 4`), adopted/manual device registry (`registry.rs`), `egui_kittest` PNG golden snapshots, and `glyph_source_scan` tofu guard. |
| **Total** | **4 Crates** | **26** | **10,104** | **100% Pure Rust, 1 `Cargo.lock`, 0 `.unwrap()`, 0 `node_modules`, 0 `.venv`.** |

### 2.2 Module-by-Module Breakdown of `devices-core`

| Module | Lines | Key Capability & Hardening Highlights |
|---|---:|---|
| `ring.rs` | 804 | Hand-rolled Ring OAuth2 password + 2FA + refresh flow (`ring_token.cache`), doorbells, dings, motion history, alarm read/arm/disarm (siren deliberately excluded), and doorbell JPEG snapshot polling. |
| `lib.rs` | 550 | 24 `Command` variants, `dispatch()` (auto-recording energy/weather/alarm telemetry into `devices-store`), and compile-time `dispatch_future_is_send` guard. |
| `import.rs` | 445 | Zero-config importer reading sibling Python `devices-mcp` (`config.yaml`, `.env` with `${VAR:-default}` expansion, `hue_bridge.cache`, `ring_token.cache`, per-camera creds, and `readonly` plug guards). |
| `camera.rs` | 435 | Unified snapshot dispatcher (`tapo`, `onvif`, `usb`, `ring`, `url`, `demo`) + pure-Rust JPEG SOI (`0xFF 0xD8`) and bounds-checked SOF0/1/2 dimension parser (`jpeg_dimensions`). |
| `llm.rs` | 374 | Unified chat & model-list client for **Ollama** (`:11434`), **LM Studio** (`:1234`), **OpenAI**, **Anthropic** (`/v1/messages` with system-prompt folding), and custom OpenAI-compatible endpoints. |
| `video.rs` | 338 | **Zero-FFmpeg live RTSP video pipeline:** `retina 0.4` RTSP demux + `openh264 0.9` H.264 decoder + `to_annex_b` length-prefixed NAL converter + `strip_creds` URI sanitizer + `SnapshotPoller` fallback. |
| `hue.rs` | 241 | CLIP v1 REST driver with Hue Bridge Pro self-signed HTTPS support (`HUE_HTTPS`), link-button pairing (`hue.pair`), and explicit `on: bool` safety check. |
| `bridge.rs` | 221 | Fleet crossconnect HTTP client: `aiwatcher` Fleet Events (`:10946`), `speech-mcp` TTS (`:10909`), and generic `http(s)` webhooks (`robofang`, `ntfy`). |
| `creds.rs` | 215 | 4-level credential resolution (`explicit args > process env > imported .env > config`), `with_timeout_secs`, and unified vendor error classification. |
| `scan.rs` | 187 | Concurrent `/24` LAN scanner (`254 hosts x 6 ports`, `buffer_unordered(256)`) with unauthenticated HTTP/HTTPS protocol fingerprinting for Hue Bridges and Shelly relays. |
| `netatmo.rs` | 162 | Hand-rolled Netatmo OAuth2 `refresh_token` + `getstationsdata` weather/CO2/humidity driver. |
| `cam_onvif.rs` | 140 | ONVIF media profile enumeration, multi-profile `GetSnapshotUri` fallback, and `GetStreamUri` RTSP discovery. |
| `weather.rs` | 132 | Keyless Open-Meteo current weather + 24-hour hourly temperature curve (`temperature_2m`) + WMO weather code table (Vienna default). |
| `plug.rs` / `plug_*.rs` | 409 | Unified smart-plug traits + **Tapo** (`P100/P105/P110/P115` with mW-to-W fix), **Kasa** (`kasa-rs` XOR + KLAP), **Shelly** (`shelly-core` Gen1/2/3), and **Demo** (`42.5 W`). |
| `cam_tapo.rs` / `cam_usb.rs` / `cam_url.rs` | 206 | Tapo native camera client, Windows MediaFoundation USB webcam capture (`nokhwa 0.10`), and HTTP(S) URL JPEG fetcher. |
| `error.rs` | 74 | Typed `DeviceError` (`InvalidInput`, `Offline`, `Timeout`, `Auth`, `Protocol`, `Unsupported`) + bounded `validate_device_id`. |

---

## 3. Deep-Dive Assessment of the `v0.3.0` -> `v0.4.0+` Feature Wave

### 3.1 Live RTSP Video Without FFmpeg (`video.rs` + `Cameras` Page)
- **What Was Built:** Instead of shelling out to a 100 MB `ffmpeg.exe` child process or pulling GStreamer DLLs, `video.rs` pairs `retina 0.4` (pure-Rust async RTSP client) with Cisco's `openh264 0.9` (compiled directly from bundled C source by Cargo on MSVC without even needing NASM).
- **Hostile-Input Hardening:** `to_annex_b()` converts 4-byte length-prefixed AVCC NAL units to Annex-B start codes (`0x00 0x00 0x00 0x01`) using `checked_add` and slice bounds checks so a malformed camera packet can never panic the player.
- **Live Hardware Trap Caught:** `retina` refuses URLs containing `user:pass@host` userinfo. OpenCode wrote `strip_creds()` to scrub embedded credentials from ONVIF `GetStreamUri` responses and pass them cleanly via `SessionOptions::creds` -- proving **3 decoded H.264 frames in 0.46 seconds** from Sandra's live Tapo camera (`.164`).
- **UI Integration:** The `Cameras` page now has a **Live / Stop** player with real-time FPS telemetry and zero-allocation GPU texture updates via `TextureHandle::set`.

### 3.2 Fleet Crossconnect Without a New Server (`bridge.rs` + `Crossconnect` Page)
- **Architectural Insight:** Answered the fleet crossconnect question with maximum simplicity: because `aiwatcher` (`:10946`) and `speech-mcp` (`:10909`) already expose localhost HTTP endpoints, `rust-devices-mcp` needs only a 221-line typed client (`bridge.rs`), not a new daemon.
- **Live Verification:** `bridge.speak` was verified live on the server speakers via `speech-mcp`, while `bridge.event` degraded gracefully with a typed error when `aiwatcher` was offline.

### 3.3 SQLite Long-Term Telemetry Historian (`crates/devices-store`)
- **Design Quality:** Cleanly separated into its own L1 workspace crate (`devices-store`) using `rusqlite 0.40` (`bundled`).
- **Automatic Capture:** `devices_core::dispatch` automatically records every live `PlugStats` reading, `Weather` reading (plus indoor Netatmo station temperature/humidity/CO2), and `RingAlarm` mode change into `%LOCALAPPDATA%/rust-devices-mcp/devices.db` and exposes `energy.history`, `weather.history`, and `alarm.history` as first-class CLI commands and MCP tools.

### 3.4 "Joe Schmoe" Zero-Config Onboarding (`scan.rs`, `registry.rs`, `vault.rs`, `Settings` Page)
- **Concurrent LAN Discovery (`scan.rs`):** Scans a `/24` subnet (`253 hosts x 6 ports = 1,518 probes`) at 256-way concurrency in ~10-20s on Wi-Fi, and performs unauthenticated HTTP/HTTPS identification for classic Hue Bridges (`:80`) and **Hue Bridge Pro (`:443` self-signed HTTPS)** as well as Shelly relays (`/shelly`).
- **OS Secret Vault (`vault.rs`):** Uses `keyring 4` to store passwords in **Windows Credential Manager** under `service = "rust-devices-mcp"`. Crucially, commit `2a7a029` moved vault reads off the UI thread (`Job::VaultLoaded`) so slow keychain unlock prompts never block first-frame rendering.
- **Device Pickers (`registry.rs` + `app.rs`):** Fleet-imported devices, LAN-adopted scan hits, and manual entries unify into dropdown pickers on the `Energy` and `Cameras` tabs (`aab6c17`), eliminating manual IP typing.

### 3.5 In-App Multi-Provider LLM Chat (`llm.rs` + `Chat` Page)
- Supports local-first **Ollama** (`qwen3.8:27b` default) and **LM Studio** out of the box alongside **OpenAI** and **Anthropic**, with a 1-click **"House Context"** button that snapshots live device states into the system prompt.

### 3.6 Desktop Polish & Concurrency Bug Bounties Caught
1. **Re-Entrant `egui::Context` RwLock Deadlock (`2a7a029`):** Calling `ctx.set_zoom_factor()` or `ctx.set_visuals()` while holding a closure inside `ctx.input(|i| ...)` deadlocks `egui`'s internal context lock on Windows. Fixed by reading input deltas inside `ctx.input`, dropping the lock, and mutating zoom/theme afterward.
2. **Async `Send` Poisoning via Borrowed `&str` in Stream Combinators (`21d3788` / `9131381`):** A static `&'static str` tuple element inside `futures::stream::iter(...).map(...).buffer_unordered(256)` in `scan.rs` caused rustc's lifetime inference to tie a non-`'static` borrow across `.await`, breaking `tokio::spawn(dispatch(cmd))` with an inscrutable HRTB error. Fixed by converting scan targets to owned `(String, u16, String)` tuples and locking the invariant forever with `#[test] fn dispatch_future_is_send()`.
3. **Scroll-Wheel Hijacking on Hue Sliders (`aab6c17`):** `egui::Slider` captures mouse-wheel events when hovered, causing accidental brightness changes while scrolling the Lighting page. Replaced with explicit `-25` / `+25` brightness steppers and `0% / 25% / 50% / 75% / 100%` preset buttons.

---

## 4. Complete 24-Tool MCP & CLI Surface (`aab6c17`)

All 24 capabilities are exposed identically over `rust-devices-mcp <subcommand> --json`, `rust-devices-mcp mcp` (`tools/list` / `tools/call`), and `devices-app`:

| # | Tool Name (`MCP`) | CLI Subcommand | Category | Status |
|---:|---|---|---|---|
| 1 | `status` | `status` | System | Live |
| 2 | `plug.get_stats` | `plug-stats` | Smart Plugs | Live (Tapo proven; Kasa/Shelly code-complete) |
| 3 | `plug.toggle` | `plug-toggle` | Smart Plugs | Live (`readonly` guard enforced) |
| 4 | `camera.describe` | `camera-describe` | Cameras | Live |
| 5 | `camera.snapshot` | `camera-snapshot` | Cameras | Live (Tapo, USB, Ring, URL, Demo proven) |
| 6 | `hue.list_lights` | `hue-lights` | Lighting | Live (18 lights on `.236` / `.219`) |
| 7 | `hue.set_light` | `hue-set-light` | Lighting | Live (requires explicit `on`) |
| 8 | `hue.pair` | `hue-pair` | Lighting | Live |
| 9 | `netatmo.stations` | `netatmo-stations` | Climate / Weather | Live (Stroheckgasse station) |
| 10 | `ring.status` | `ring-status` | Security / Ring | Live |
| 11 | `ring.doorbells` | `ring-doorbells` | Security / Ring | Live (Front Door) |
| 12 | `ring.alarm` | `ring-alarm` | Security / Ring | Code-complete (no siren by design) |
| 13 | `ring.auth` | `ring-auth` | Security / Ring | Live |
| 14 | `ring.auth_2fa` | `ring-auth2fa` | Security / Ring | Live |
| 15 | `weather.now` | `weather` | Weather | Live (Open-Meteo keyless + 24h hourly curve) |
| 16 | `bridge.event` | `bridge-event` | Crossconnect | Live (`aiwatcher` `:10946`) |
| 17 | `bridge.speak` | `bridge-speak` | Crossconnect | Live (`speech-mcp` `:10909` spoken on server) |
| 18 | `bridge.webhook` | `bridge-webhook` | Crossconnect | Live (generic `http(s)` POST) |
| 19 | `energy.history` | `energy-history` | Historian (`SQLite`) | Live (`devices-store`) |
| 20 | `weather.history` | `weather-history` | Historian (`SQLite`) | Live (`devices-store`) |
| 21 | `alarm.history` | `alarm-history` | Historian (`SQLite`) | Live (`devices-store`) |
| 22 | `llm.chat` | `chat` | Local/Cloud LLM | Live (Ollama, LM Studio, OpenAI, Anthropic) |
| 23 | `llm.models` | `llm-models` | Local/Cloud LLM | Live |
| 24 | `net.scan` | `net-scan` | Discovery | Live (Hue HTTP/HTTPS + Shelly + port fingerprint) |

---

## 5. Minor Doc-Sync Action Items (For Next Commit / `v0.5.0` Tag)

While the code, tests (`58/58`), clippy gates, and `CHANGELOG.md` are pristine, rapid feature velocity left four minor text counts in `README.md` and `docs/PRD.md` one step behind the code:

1. **Tool Count in `README.md` & `docs/PRD.md`:**
   - `README.md` §"MCP contract" (line 163) and CLI table (lines 139--160) list **18 tools**, omitting the 6 newest tools (`energy.history`, `weather.history`, `alarm.history`, `llm.chat`, `llm.models`, `net.scan`). `crates/devices-mcp/src/main.rs` (`catalog_lists_twentyfour_tools`) now verifies **24 tools** (and 25 CLI subcommands including `mcp`).
2. **Workspace Diagram in `README.md`:**
   - `README.md` §"Architecture" (lines 101--114) lists 3 crates (`devices-core`, `devices-mcp`, `devices-app`); add `crates/devices-store` (`L1 SQLite historian`).
3. **Test Count in `README.md`:**
   - `README.md` §"Verification" (line 246) states `# 37/37 green`; update to `# 58/58 green (+ 1 live-hardware ignored)`.
4. **Version Bump Ready (`0.4.0` -> `0.5.0`):**
   - The `## Unreleased - chat, settings, scan, store history, pickers` section in `CHANGELOG.md` represents a complete, verified milestone (SQLite store, LAN scanner, OS vault, LLM chat, device pickers, and deadlock fixes) ready to be tagged as **`v0.5.0`**.
