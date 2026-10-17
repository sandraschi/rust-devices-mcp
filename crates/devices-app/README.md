# devices-app (L3)

egui immediate-mode desktop dashboard calling `devices-core` **in-process**.
No WebView2, no JavaScript, one `cargo` toolchain — the ArtCraft native stack
per `RUST_ARTCRAFT_STANDARD.md` (Tauri was prototyped, then removed).

## Layout (mirrors the original webapp)

Retractable left sidebar — Dashboard, Energy, Lighting (with per-light
sub-entries), Climate (honest placeholder), Cameras, Weather, Alarms (with
per-doorbell sub-entries), Raw JSON — and the selected page in the main area.
Per-device detail pages for Hue lights and Ring doorbells. High-contrast theme
(near-white on slate-950) with a 16px base font.

Field values persist to `%LOCALAPPDATA%/rust-devices-mcp/ui.json` across
restarts. **Passwords never persist** — only IDs, IPs, and selections.

## Data flow

Every button maps to a `Command` variant. Async drivers run on a shared Tokio
runtime; results cross into the UI thread over mpsc and the UI repaints while
jobs are pending (spinner + counter in the top bar). Snapshots decode to RGB
on the worker and become GPU textures on the UI thread.

First paint is never empty: status + keyless weather + (when env provides)
Hue lights and Ring state load at boot.

## Headless CI

`devices-app --smoke` runs status + demo plug + demo snapshot through the
same dispatch with no window. Exit 0 = parity holds.
