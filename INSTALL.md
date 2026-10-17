# Install — rust-devices-mcp v0.1.0

Two self-contained Windows x64 binaries (~43 MB total) plus an installer.
No runtime, no admin rights. Uninstall = delete the files (or Add/Remove Programs).

## 1. Download

From the [latest release](https://github.com/sandraschi/rust-devices-mcp/releases/latest):

- `Devices-Rust-<ver>-setup.exe` (~17 MB) — installer: Start Menu entry,
  Add/Remove Programs + uninstaller. Easiest path.
- `rust-devices-mcp.exe` (~11 MB) — CLI + MCP server. **This is the tool.**
- `devices-app.exe` (~31 MB, self-contained) — the **egui desktop dashboard**:
  double-click, no install. Sidebar mirrors the webapp (Dashboard, Energy,
  Lighting, Cameras, Weather, Alarms, Crossconnect, Help). Fields persist across restarts
  (passwords never do). `devices-app.exe --smoke` self-tests without a window.
- `SHA256SUMS.txt` — verify with:
  `Get-FileHash .\rust-devices-mcp.exe -Algorithm SHA256` and compare.

Put both exes somewhere permanent, e.g. `C:\Tools\rust-devices-mcp\`
(not Downloads — step 3 points other apps at this path).

> **SmartScreen:** the binaries are unsigned, so Windows will warn on first
> run (*More info → Run anyway*). Expected, not a compromise — signing lands
> with the tier-4 NSIS installer.

## 2. Prove it runs (30 seconds)

```powershell
cd C:\Tools\rust-devices-mcp
.\rust-devices-mcp.exe status
.\rust-devices-mcp.exe plug-stats --device-ip 192.168.1.50 --json
.\rust-devices-mcp.exe plug-toggle --device-ip 192.168.1.50 --state false
.\rust-devices-mcp.exe camera-describe --device-ip 192.168.1.51 --json
```

> ⚠️ **Honest-driver note:** most drivers are REAL (Tapo plugs/cams, Hue,
> Ring, Netatmo, USB, weather — all proven live, see README). `--vendor demo`
> / `--source demo` remain deterministic fakes (42.5 W / stub JPEG) for CI
> and offline probes. Kasa/Shelly have no hardware on the reference LAN.

## 3. Credentials (v0.2+ real drivers)

Explicit flags win; environment is the fallback; nothing is ever logged:

| Vendor | Env vars |
|---|---|
| Tapo plugs | `TAPO_EMAIL`/`TAPO_USERNAME`/`TAPO_P115_EMAIL`/`TAPO_LIGHTING_EMAIL` + matching `*_PASSWORD` |
| Tapo cameras | above (handshake) + `CAMERA_USERNAME`/`CAMERA_PASSWORD` (or `TAPO_CAMERA_*`, `TAPO_CAMERA_2_*` per camera) |
| Kasa | `KASA_USERNAME` + `KASA_PASSWORD` (legacy XOR needs none) |
| Shelly | `SHELLY_PASSWORD` (optional) |
| Hue | `HUE_BRIDGE_IP` + `HUE_USERNAME` (or `HUE_BRIDGE_USERNAME`; pair once: `hue-pair` after pressing the link button) |
| Netatmo | `NETATMO_CLIENT_ID` + `NETATMO_CLIENT_SECRET` + `NETATMO_REFRESH_TOKEN` |
| Ring | `RING_EMAIL` + `RING_PASSWORD` (first login, maybe 2FA) or token cache (`RING_TOKEN_FILE`, default `ring_token.cache`) |
| ONVIF | `CAMERA_USERNAME` + `CAMERA_PASSWORD`, `--onvif-port` (default 2020) |
| Bridge | `AIWATCHER_BASE_URL` (default :10946), `SPEECH_BASE_URL` (default :10909) |

On a fleet machine the app imports all of this from the devices-mcp
checkout automatically (see README “Fleet import”) — env vars are the
override path, and what CLI/MCP users set.

```powershell
$env:TAPO_EMAIL = "you@example.com"; $env:TAPO_PASSWORD = "..."
.\rust-devices-mcp.exe plug-stats --vendor tapo --device-ip 192.168.0.17 --json
.\rust-devices-mcp.exe camera-snapshot --source usb --device-index 0 --out snapshot.jpg
```

## 4. Use it as an MCP server

Any stdio-capable MCP client works: command = path to the exe, args = `["mcp"]`.
No ports, no daemon to start, logs on stderr, JSON-RPC on stdout.

**Claude Desktop:** back up then edit
`%APPDATA%\Claude\claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "rust-devices": {
      "command": "C:\\Tools\\rust-devices-mcp\\rust-devices-mcp.exe",
      "args": ["mcp"]
    }
  }
}
```

Fully quit Claude Desktop (tray icon too), restart, and look for the tools
icon: 18 tools (`status`, `plug.get_stats`, `plug.toggle`,
`camera.describe`, `camera.snapshot`, `hue.*`, `netatmo.stations`,
`ring.*`, `weather.now`, `bridge.*`). Try: *"What is the power draw of
the plug at 192.168.1.50?"* (needs Tapo creds or `--vendor demo` for the
canned 42.5 W).

**Claude Code:**

```powershell
claude mcp add rust-devices -- C:\Tools\rust-devices-mcp\rust-devices-mcp.exe mcp
```

**OpenCode:** same command + `["mcp"]` in your MCP server config.

**Manual probe** (no client needed):

```powershell
'{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | .\rust-devices-mcp.exe mcp
```

## 5. Troubleshooting

| Symptom | Cause / fix |
|---|---|
| SmartScreen block | Unsigned alpha binary — *More info → Run anyway* |
| Client shows no tools | Config path typo, or Claude Desktop wasn't fully restarted |
| Garbage on stdout | Nothing should log to stdout; report it as a bug (stdio invariant) |
| `auth failed: …` | Missing secret — the error names the exact env var; see table above |
| aiwatcher event fails | aiwatcher backend (`:10946`) isn't running — start it first; the error is graceful |
| Live video stalls | Camera RTSP unreachable or wrong creds; snapshots still work — check IP/user/pass |

## 6. Uninstall / upgrade

Delete the folder. For a new release, overwrite the exes in place — client
configs keep working because the path doesn't change.
