# UI Parity Flow — cloning a beautiful webapp into egui, reproducibly

For the coming 50 rustifications: the Python webapps are the beauty oracle,
the egui app is the clone. This doc is the mechanical loop that makes repo
#50 cost a fraction of repo #1. It mirrors ArtCraft's oracle verification
(headless screenshots as ground truth), applied to UI instead of pixels.

## The loop (per page, per device)

```text
1. ORACLE   screenshot the Python webapp page (Playwright, app running)
2. CLONE    build the egui page against the same live data
3. CAPTURE  screenshot the egui window (CUA window capture, exe running)
4. COMPARE  side-by-side in docs/screenshots/<page>-oracle.png vs -clone.png
5. CONVERGE  adjust Visuals/Frame/layout until Sandra signs off
6. LOCK     kittest widget snapshots guard the converged look in CI
```

Beauty is human-judged (Sandra is the oracle's oracle); machines guard
against *regression*, never approve *taste*.

## 1. Oracle screenshots (Python webapp)

Prerequisites: backend + frontend running (the webapp's own start script).

```powershell
# one shot per route, 1280x860 to match the egui window
npx playwright screenshot --viewport-size=1280,860 \
  --wait-for-timeout=3000 http://127.0.0.1:<frontend-port>/lighting \
  docs/screenshots/lighting-oracle.png
```

Routes worth oracling (devices-mcp set): `/` dashboard, `/energy`,
`/lighting`, `/cameras`, `/weather`, `/alarms`. Save as
`docs/screenshots/<page>-oracle.png`. These files are ground truth — never
edit them, never compress them.

## 2. Clone in egui (this repo's rules)

- One `Page` variant per webapp route; per-device detail pages keyed by id.
- Sidebar mirrors the webapp's nav tree (same order, same labels).
- Decoration primitives only: `Visuals` (theme), `Frame` (cards: fill +
  rounding + stroke + shadow), `RichText` (spans, pills via
  `background_color`), `ProgressBar`/`Slider`/`Image` (GPU textures).
- No color emoji in the default font stack — geometric glyphs only
  (● ○ ◆ ▸ ▾ ✓ ⚠ ★). Tofu boxes are a parity failure.
- Base font 16px, headings 23px, near-white (#F2F6FC) on slate-950 (#020612).

## 3. Clone screenshots (running exe)

No headless GPU renderer is assumed. Capture the real window:

```powershell
# focus + capture the app window to PNG (see scripts/capture-window.ps1)
.\scripts\capture-window.ps1 -ProcessName devices-app -Out docs/screenshots/dashboard-clone.png
```

Click through every page (sidebar order), one PNG per page. Same 1280x860
window size as the oracle shots — resize once, keep it.

## 4. Compare

Side-by-side, human eyes first. Machine pre-check where useful (sizes,
presence of key regions) — but a pixel diff that "passes" while looking
wrong is a failure of the check, not a pass. Record verdicts in the PR or
CHANGELOG line (`parity: lighting 4/5 — slider styling pending`).

## 5. Converge checklist (the usual suspects, cheapest first)

1. Font sizes + text colors (80% of "horrible contrast" complaints).
2. Spacing/padding (egui defaults run tight — `item_spacing`, card margins).
3. Card fills/rounding/shadows (the webapp's `rounded-xl border-slate-800`).
4. Accent colors per domain (energy amber, hue yellow, weather sky…).
5. Pills/badges for state (ON/OFF, reachable, modes).
6. Layout order (same information hierarchy as the oracle page).
7. Empty states (every page must say something honest with no data, never blank).

## 6. Lock with kittest (CI, no GPU)

`egui_kittest` renders widgets headlessly in `cargo test` and snapshots PNGs
(see `crates/devices-app/src/app.rs` `#[cfg(test)]`). These guard *components*
(cards, pills, KPI row, light row) against regression. They do not replace
step 4 — full pages need the GPU window.

```powershell
cargo test -p devices-app          # runs kittest snapshots
```

Snapshots live next to the tests that own them. A failing snapshot means the
look changed: inspect the PNG, and either fix the code or bless the new gold.

## Later: cloning from an exe alone

When only a third-party `.exe` exists (no source, no webapp), steps 1–2
change: oracle shots come from CUA window capture of the running exe
(same `capture-window.ps1`), and the command bus is reverse-engineered from
observed behavior into `Command` variants first (ArtCraft gap scorecard),
UI second. Everything downstream (3–6) is identical.
