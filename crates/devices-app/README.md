# devices-app (L3)

Desktop shell: Tauri v2 frontend calling `devices-core` **in-process**.
No child process, no localhost HTTP hop, no JSON-RPC round-trip for the GUI —
the desktop path skips L2 entirely. That missing hop is the whole point of the
Rust-only topology: Tauri doesn't need to launch an external engine to talk to
its own engine.

## The deal (v0.1)

- **Dependencies:** `devices-core` + `serde_json`. The `tauri` crate itself is
  **not** wired yet (scorecard tier-4) — deliberately, so v0.1 proves
  core+MCP+CLI with a minimal dep tree that builds anywhere.
- Each `*_command` function is already written handler-shaped: synchronous,
  in-process, straight into `dispatch`. When the `tauri` crate lands, the
  function bodies don't change — only the registration does:

```rust
// Tier-4 wiring (signatures already final):
tauri::Builder::default()
    .invoke_handler(tauri::generate_handler![
        plug_get_stats_command,
        plug_toggle_command,
        camera_describe_command,
    ])
```

- `tauri.conf.json` ships as a tier-4 placeholder: NSIS target, `com.sandraschi`
  identifier, single-binary bundle, strict CSP. The `_note` field says exactly
  what v0.1 proves and what tier-4 adds.
- The `main()` headless smoke proves the desktop path in CI today (no webview
  needed): same calls the Tauri handlers will make, asserting identical values.

## Desktop/MCP parity proof

The unit test asserts `plug_toggle_command(...) == dispatch(Command::PlugToggle
{...})` — the GUI and the agent surface are the same function call. If they
ever diverge, the test goes red. That's the command-bus guarantee made executable.

## Tier-4 checklist (when we get there)

1. Add `tauri` crate + `ui/dist` frontend (static Vite build or Leptos/Dioxus —
   no Node at runtime either way).
2. `just package-nsis` (recipe to be added) → installer size + cold-boot
   shootout vs. the legacy Python bundle for the README table.
3. Screenshot the Tauri window for the README Preview section (fleet standard).

Run the smoke: `cargo run -q -p devices-app`.
