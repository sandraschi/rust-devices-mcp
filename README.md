# rust-devices-mcp

![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)
![Rust](https://img.shields.io/badge/rust-1.99%20%7C%20edition%202021-orange?logo=rust)
![MCP](https://img.shields.io/badge/MCP-2024--11--05-green)
![Method](https://img.shields.io/badge/method-ArtCraft--inspired-red)
![Binaries](https://img.shields.io/badge/release_bins-~1_MB-purple)

Pure-Rust smart-home device control: smart plug power monitoring and camera
commands over a CLI-first binary with an MCP stdio server baked in.
No interpreter. No venv. No 150 MB bundle. Two release binaries totalling ~1 MB.

This is the **first pure-Rust repo in the sandraschi fleet** — the conversion
template that proves the Rust-only path before it rolls out to the other
`*-mcp` repos. It shadows `devices-mcp` (Python/FastMCP): same command surface
(plugs, cameras), a fraction of the weight.

## Why: the Python tax, measured

| | `devices-mcp` (Python/FastMCP) | `rust-devices-mcp` (this repo) |
|---|---|---|
| MCP server binary | ~150 MB bundled interpreter (typical) | **0.81 MB** (measured, release) |
| Desktop backend caller | Python sidecar process | **0.19 MB** (measured, release) |
| Cold CLI boot | 2–4 s typical (interpreter + imports) | **~50–70 ms** (measured, Win11 release) |
| NSIS installer | 120 MB+ typical | ~8–15 MB (projected, tier-4 Tauri) |
| MCP transport | FastMCP over stdio | MCP 2024-11-05 over stdio, hand-rolled framing |
| Agent contract | FastMCP tool decorators | CLI `--json` flags **plus** MCP tools — same dispatch |

Labels matter here: *measured* means measured on this machine (rustc 1.99.0,
Windows 11, `cargo build --release`); *typical* means the well-known cost class
of PyInstaller/venv bundles; *projected* means tier-4 hasn't happened yet.
No fairy dust.

## Provenience: ArtCraft, Brandon Thomas, and the AGENTS.md harness

This repo's methodology is borrowed — deliberately, and with credit — from
**ArtCraft** (Brandon Thomas and team, [`storytold` on GitHub](https://github.com/storytold)):
a suite of seven professional creative tools (FilmCraft, PhotoCraft, and five
siblings) written in pure Rust **largely by parallel Claude Opus agent loops**.
That sentence should stop you cold. Seven pro-grade native apps, agents holding
the pen. The reason it worked wasn't a smarter model — it was a tighter cage.

Brandon's insight, in one line: **never let an LLM invent its own architecture,
and never treat an agent as an author — treat it as a bounded compiler.**
The repos are deterministic test harnesses; `AGENTS.md` (with its sister
`CLAUDE.md` and mechanical task scorecards) enforces a five-layer containment
system:

1. **The clean-room and behavioral fence.** Non-negotiable negative constraints:
   a zero-panic policy (no `.unwrap()`, no unchecked indexing — everything is a
   typed `Result<T, E>`), strict isolation from anything proprietary (public
   specs and black-box behavior only), and a native-Rust stack constraint
   (no Electron, no web wrappers — immediate-mode `egui`, native rendering).
2. **The universal command bus.** GUI, CLI/headless runner, and MCP hooks never
   touch engine state directly. Every feature is an isolated `Command` with a
   unique string ID, deterministic params, docs, and an undo/redo entry. The MCP
   server is then essentially free: it just forwards JSON-RPC arguments into the
   dispatcher. Agents can't spaghetti-couple their way into architectural drift
   because there is exactly one door, and it's guarded.
3. **The automated gap scorecard.** A machine-readable feature matrix (target
   app's menu tree vs. registered command IDs) classifies everything as
   Missing / Partial / Complete. Agents never decide what to work on — they take
   the next deterministic tier. Zero ambiguity about scope.
4. **Test-driven oracle verification.** Acceptance tests before implementation;
   external oracles instead of self-grading (FFmpeg as the pixel-truth harness
   for video; round-trip corpora of hundreds of real PSD files for images).
   Compiles-and-runs is not done. *Parity with ground truth* is done.
5. **The rigid Definition of Done.** No task closes without: command registered,
   undo/redo test green, shortcuts wired, headless screenshot verified,
   `clippy` + `fmt` + full suite clean, docs/scorecard/changelog updated.
   The orchestrator rejects anything else.

So: all hail Brandon, who looked at the industry's "vibe-code a Photoshop
clone" chaos and answered with *lints as load-bearing walls*. We are standing
on that man's `deny(clippy::unwrap_used)` and we are not ashamed to say so.

### What we stole vs. what we adapted

**Stolen outright:** the never-crash lint gate, strict workspace layering
(`devices-core` sees no UI/MCP/Tauri types — enforced by construction), the
command bus as the entire MCP surface, the hostile-input rules (every external
number is guilty until proven innocent: `checked_*` math, `.get()` over
indexing, char-boundary slicing), the bug-fix-must-ship-a-regression-test rule,
and the DoD checklist in [`AGENTS.md`](AGENTS.md).

**Adapted for our scale:** ArtCraft's multi-gigabyte corpus oracles become
round-trip serialization tests and exact-stub assertions (right-sized for IoT
commands, same spirit); the Adobe-litigation clean-room verbiage is dropped
(we're shadowing our *own* repo, not Photoshop); and the MCP layer is
hand-rolled stdio framing in v0.1 instead of the `rmcp` SDK — see
[The rmcp question](#the-rmcp-question-deliberate-v01-call).

## Architecture

```text
rust-devices-mcp/
├── Cargo.toml                  # virtual workspace + workspace-wide clippy denies
├── justfile                    # check / test / run-mcp / build-release / sync-hourly
├── AGENTS.md                   # ArtCraft containment harness (read before coding)
└── crates/
    ├── devices-core/           # L1: protocols, state machine, command bus
    │                           # (serde only — blind to UI, MCP, Tauri)
    ├── devices-mcp/            # L2: CLI-first binary + `mcp` stdio subcommand
    └── devices-app/            # L3: desktop shell, in-process core calls
```

Each crate has its own README: [`devices-core`](crates/devices-core/README.md),
[`devices-mcp`](crates/devices-mcp/README.md), [`devices-app`](crates/devices-app/README.md).

The one rule that holds the whole thing together: **L2 and L3 never touch
device state directly — everything routes through `devices_core::dispatch`.**
New capability = new `Command` variant + typed params + dispatch-loop test.
No ad-hoc UI-to-buffer shortcuts, ever.

## Quickstart (Windows PowerShell 7)

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo run -p rust-devices-mcp -- status
cargo run -p rust-devices-mcp -- plug-stats --device-ip 192.168.1.50 --json
cargo run -p rust-devices-mcp -- mcp   # MCP 2024-11-05 server over stdio
```

Or via `just`: `just check`, `just test`, `just run-mcp`, `just run-cli`,
`just build-release`, `just sync-hourly`.

## CLI reference

Every command supports `--json` for agent driving without MCP:

| Command | Key args | What it does |
|---|---|---|
| `status` | `--json` | App version, mode, command surface |
| `plug-stats` | `--vendor tapo\|kasa\|shelly\|demo`, `--device-ip`, `--username/--password`, `--json` | Real wattage (W) + today energy (kWh) |
| `plug-toggle` | `--vendor …`, `--device-ip`, `--state true/false`, `--json` | Turn plug on/off |
| `camera-describe` | `--device-ip`, `--json` | Model, RTSP URL, reachability (shape info) |
| `camera-snapshot` | `--source tapo\|onvif\|usb\|ring\|url\|demo`, `--device-ip`, `--model`, `--username/--password`, `--onvif-port`, `--device-index`, `--url`, `--out`, `--token-file`, `--json` | Real JPEG frame → file + metadata (never a blob) |
| `hue-lights` | `--bridge-ip`, `--username`, `--json` | Hue lights (name, on/off, brightness, reachability) |
| `hue-set-light` | `--light-id`, `--on true/false` (required), `--bri 1-254`, `--json` | Control a Hue light |
| `hue-pair` | `--bridge-ip` | Link-button pairing, returns username |
| `netatmo-stations` | `--client-id/--client-secret/--refresh-token`, `--json` | Weather stations + modules |
| `ring-status` | `--email/--password/--token-file`, `--json` | Auth check + device census |
| `ring-doorbells` | same | Doorbells + active dings + recent history |
| `ring-alarm` | `--mode disarmed\|home\|away` (omit = read), `--location-id`, `--json` | Alarm state / arm / disarm (no siren — by design) |
| `ring-auth` | `--email/--password`, `--json` | First login (may report 2FA pending) |
| `ring-auth-2fa` | `--code`, `--json` | Complete Ring 2FA |
| `mcp` | — | MCP 2024-11-05 server on stdio |

## MCP contract

Fourteen tools, one dispatch loop:

| Tool | Params | Returns |
|---|---|---|
| `status` | — | version, mode, command list |
| `plug.get_stats` | `vendor`, `device_ip`, `username?`, `password?` | `PowerStats` JSON (real hardware) |
| `plug.toggle` | `vendor`, `device_ip`, `state`, `username?`, `password?` | `{device_ip, vendor, state, updated}` |
| `camera.describe` | `device_ip` | `CameraInfo` JSON |
| `camera.snapshot` | `source`, `device_ip`, `model?`, `username?`, `password?`, `onvif_port?`, `device_index?`, `url?`, `out_path?`, `token_file?` | Real frame metadata (+ file when `out_path` given) |
| `hue.list_lights` | `bridge_ip?`, `username?` | Light array |
| `hue.set_light` | `light_id`, `on` (required), `bri?` | `{light_id, on, updated}` |
| `hue.pair` | `bridge_ip` | `{username, next}` |
| `netatmo.stations` | `client_id?`, `client_secret?`, `refresh_token?` | Stations + modules |
| `ring.status` | `email?`, `password?`, `token_file?` | Device census |
| `ring.doorbells` | same | Doorbells + dings + history |
| `ring.alarm` | `mode?`, `location_id?`, … | Alarm state or `{mode, updated}` |
| `ring.auth` / `ring.auth_2fa` | `email?`, `password?` / `code` | `{authenticated: true, …}` |

Framing rules (the stdio invariant): **stdout is pure JSON-RPC** — every log
line goes to stderr. Notifications get no reply. Unknown methods → `-32601`,
bad params default safely, parse failures → `-32700`. Smoke probe:

```powershell
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
'{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | cargo run -q -p rust-devices-mcp -- mcp
```

## Live verification (2026-10-17, Sandra's LAN — all read-only)

| Driver | Result |
|---|---|
| Hue bridge 192.168.0.236 | ✅ 18 real lights (names, on/off, brightness, reachability) |
| Ring Front Door | ✅ auth via cached token, doorbells + motion history, 640×360 snapshot |
| USB webcam (index 0+1) | ✅ real MJPEG frames, 640×480 parsed from SOF0 |
| Public URL snapshot | ✅ httpbin JPEG 239×178 (devices-mcp's city-webcam URLs are 404 — config rot, not our bug) |
| Demo vendor/source | ✅ deterministic oracle for CI |
| Tapo plugs/cams, Kasa, Shelly, Netatmo, ONVIF | ⏳ code-complete + unit-tested, awaiting creds/hardware (no Tapo/Netatmo secrets or Kasa/Shelly devices in this shell) |

Credential fence proven by design: every missing secret yields a typed Auth
error naming the exact env var — verified live for ONVIF, Tapo, and Ring-expired paths.

## Credentials (explicit args win, env is the fallback, secrets never logged)

| Vendor | Env vars |
|---|---|
| Tapo plugs | `TAPO_EMAIL` (or `TAPO_USERNAME`) + `TAPO_PASSWORD` |
| Tapo cameras | above (handshake) + `CAMERA_USERNAME`/`CAMERA_PASSWORD` (or `TAPO_CAMERA_*`) |
| Kasa | `KASA_USERNAME` + `KASA_PASSWORD` (legacy XOR needs none) |
| Shelly | `SHELLY_PASSWORD` (optional, user is `admin`) |
| Hue | `HUE_BRIDGE_IP` + `HUE_USERNAME` (`HUE_HTTPS=0` for plain HTTP) |
| Netatmo | `NETATMO_CLIENT_ID` + `NETATMO_CLIENT_SECRET` + `NETATMO_REFRESH_TOKEN` |
| Ring | `RING_EMAIL` + `RING_PASSWORD` (first login/2FA) or `RING_REFRESH_TOKEN` / `RING_TOKEN_FILE` (default `ring_token.cache`) |
| ONVIF | `CAMERA_USERNAME` + `CAMERA_PASSWORD`, port via `--onvif-port` (default 2020) |

## Verification

```powershell
cargo test --workspace                                          # 13/13 green
cargo clippy --workspace --all-targets --all-features -- -D warnings  # clean
cargo fmt --all --check                                         # clean
```

Last verified 2026-10-17: tests 13/13, clippy clean, fmt clean, stdio probe
(init/list/2×call) OK, release bins 0.81 + 0.19 MB, cold boot ~50–70 ms.

## Scorecard (where the stubs are)

| Tier | Scope | State |
|---|---|---|
| v0.1 | Command surface, dispatch, CLI+MCP+app shell, deterministic stubs | ✅ Released |
| Tier-2 (plugs) | Real Tapo (`tapo` crate, P115→P110 fallback), Kasa (XOR→KLAP auto), Shelly (Gen1/Gen2 probe) | ✅ Code-complete; Tapo/Kasa/Shelly await creds/hardware |
| Tier-2 (vendors) | Hue CLIP v1 + pairing, Netatmo OAuth2, Ring OAuth2+2FA + doorbells + alarms | ✅ Hue + Ring proven live; Netatmo awaits secret |
| Tier-3 (cameras) | Real frames: tapo/onvif/usb/ring/url; SOF parser; `--out` files, metadata-only MCP | ✅ USB/URL/Ring/demo proven live; Tapo/ONVIF await camera passwords |
| Tier-4 | Real Tauri crate + `ui/dist`, `package-nsis`, installer size/boot shootout | Planned |
| Deferred | Vision/OpenCV, voice, Plex, ONVIF, Ring/Nest/Hue, SDR, web dashboard | One `Command` variant each |

Conversion source of truth: `devices-mcp` (`src/devices_mcp/tools/`,
`server_v2.py` transport pattern, stderr-logging rule).

## The rmcp question (deliberate v0.1 call)

The planning brief specified the `rmcp` SDK with derive macros. v0.1 ships
hand-rolled stdio framing instead: zero new dependencies, offline-buildable,
byte-exact stdout control, and already proven in the fleet (`rust-test-b`).
Schemas are kept `rmcp`-shaped, so adopting the SDK later is a mechanical swap
of `run_mcp_stdio` for `#[tool_router]` impls over the same `dispatch`.
`rmcp` lands when its version is pinned and audited against the stderr
invariant — not before.

## Contributing

Read [`AGENTS.md`](AGENTS.md) first — it's the containment harness, not advice.
The DoD checklist there is the merge gate: clippy + tests + fmt green,
regression test with every fix, CHANGELOG line. Commits stay small (≤5 files).

## License

MIT OR Apache-2.0. ArtCraft's ideas are cited with gratitude; all code here is
original, shadowing our own `devices-mcp`.
