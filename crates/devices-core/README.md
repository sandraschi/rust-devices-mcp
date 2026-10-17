# devices-core (L1)

Device protocols, state machine, and the command bus. The foundation crate of
`rust-devices-mcp` — and the one with the strictest rules, because everything
above it inherits its discipline.

## The deal

- **Dependencies:** `serde` + `serde_json` only. No tokio, no UI, no MCP, no
  Tauri types. This crate must compile on anything that speaks Rust — that
  blindness to its callers is what keeps agent-driven changes from
  spaghetti-coupling into protocol handlers (ArtCraft layering rule).
- **Never crash:** workspace denies on `unwrap_used`, `expect_used`, `panic`,
  `unimplemented`, `todo`. Every fallible function returns
  `Result<T, DeviceError>` and bubbles with `?`.
- **Hostile input fence:** every external string passes `require_device_id`
  (trimmed, non-empty, ≤253 chars); integer math on sizes uses `checked_*`;
  no direct indexing — `.get()` with a typed error on `None`.

## Modules

| Module | Contents |
|---|---|
| `error.rs` | `DeviceError::{Timeout, Protocol, Offline, InvalidInput}` + `Display` + `require_device_id` fence |
| `plug.rs` | `PowerStats`, `PlugController::get_stats/toggle` (Tapo P115 / Tuya shape) |
| `camera.rs` | `CameraInfo`, `SnapshotMeta`, `CameraController::describe/snapshot_meta` (RTSP shape) |
| `lib.rs` | `Command` enum + `dispatch()` — the single door every caller walks through |

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
