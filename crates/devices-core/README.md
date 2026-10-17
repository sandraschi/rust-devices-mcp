# devices-core (L1)

Device protocols, state machine, and the command bus. The foundation crate of
`rust-devices-mcp` — and the one with the strictest rules, because everything
above it inherits its discipline.

## The deal

- **Dependencies:** device-protocol crates only (`tapo`, `kasa-rs`,
  `shelly-core`, `oxvif`, `nokhwa`, `reqwest`, `tokio` …). No UI, no MCP, no
  Tauri types. This crate must stay blind to its callers — that blindness is
  what keeps agent-driven changes from spaghetti-coupling into protocol
  handlers (ArtCraft layering rule).
- **Never crash:** workspace denies on `unwrap_used`, `expect_used`, `panic`,
  `unimplemented`, `todo`. Every fallible function returns
  `Result<T, DeviceError>` and bubbles with `?`.
- **Hostile input fence:** every external string passes `require_device_id`
  (trimmed, non-empty, ≤253 chars); integer math on sizes uses `checked_*`;
  no direct indexing — `.get()` with a typed error on `None`.
- **Secrets:** credentials resolve explicit-args → env → typed `Auth` error.
  Never logged, never in errors (see `creds.rs`).

## Modules

| Module | Contents |
|---|---|
| `error.rs` | `DeviceError::{Timeout, Protocol, Offline, InvalidInput, Auth, Unsupported}` + fences |
| `creds.rs` | `Vendor` parse, `Credentials` resolution, timeouts, shared error classifier |
| `plug.rs` | `PowerStats` (+ vendor field), `PlugController` dispatch to vendors |
| `plug_tapo.rs` / `plug_kasa.rs` / `plug_shelly.rs` | Real drivers (P115→P110 fallback; XOR→KLAP auto; Gen1/Gen2 probe) |
| `hue.rs` | CLIP v1 lights + link-button pairing (hand-rolled) |
| `netatmo.rs` | OAuth2 refresh + stations summary (hand-rolled) |
| `ring.rs` | OAuth2 + 2FA + doorbells/dings/history + alarms + snapshots (hand-rolled, mirrors `ring_doorbell`) |
| `camera.rs` | `CamSource`, real `snapshot()` (SOI verify, SOF parse, file write), `describe` shape info |
| `cam_tapo.rs` / `cam_onvif.rs` / `cam_usb.rs` / `cam_url.rs` | Real frame grabbers (MJPEG + RGB fallback for USB) |
| `lib.rs` | `Command` enum (14 commands) + async `dispatch()` — the single door |

## The command bus

```rust
pub enum Command {
    Status,
    PlugStats { device_ip: String },
    PlugToggle { device_ip: String, state: bool },
    CameraDescribe { device_ip: String },
    CameraSnapshot { device_ip: String },
}

pub fn dispatch(cmd: Command) -> Value;
```

`dispatch` never panics: failures become `{"error": "..."}` JSON values so the
MCP layer frames them without branching. New capability = new variant +
dispatch arm + dispatch-loop unit test. String IDs (`plug.get_stats`, …) are
the contract shared with `tools/list`.

## v0.1 stubs vs. real drivers

The controllers are deterministic stubs behind the real surface — exact values
(`42.5 W`, `0.85 kWh`, 1280×720 class) so oracle tests assert parity, not
vibes. The socket/HTTP work lands here next (scorecard tier-2/3) **without
changing any caller**: L2 and L3 only ever call `dispatch`.

## Tests

9 unit tests: error display + fence rejection, JSON round-trips, checked
snapshot math, hostile-empty-IP-becomes-error-value (the never-panic proof).
Run: `cargo test -p devices-core` (from workspace root: `cargo test --workspace`).
Every bug fix must add the regression test that fails before it — ArtCraft rule.
