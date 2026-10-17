# Install — rust-devices-mcp v0.1.0

Two self-contained Windows x64 binaries, ~1 MB total. No installer, no
runtime, no admin rights. Uninstall = delete the files.

## 1. Download

From the [v0.1.0 release](https://github.com/sandraschi/rust-devices-mcp/releases/tag/v0.1.0):

- `rust-devices-mcp.exe` (0.81 MB) — CLI + MCP server. **This is the tool.**
- `devices-app.exe` (0.19 MB) — headless smoke demo (prints JSON, exits; the
  real desktop window arrives in tier-4).
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

> ⚠️ **Honest-stub warning:** v0.1 drivers are deterministic fakes.
> `plug-stats` reports **42.5 W / 0.85 kWh for any IP**, `plug-toggle` flips
> nothing (`updated: true` is theater). This release proves the plumbing —
> parsing → dispatch → JSON, identical over CLI and MCP — not the hardware
> handshake. Real Tapo drivers are tier-2 (see README scorecard).

## 3. Use it as an MCP server

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
icon: `status`, `plug.get_stats`, `plug.toggle`, `camera.describe`,
`camera.snapshot_meta`. Try: *"What is the power draw of the plug at
192.168.1.50?"* (expect the canned 42.5 W — see stub warning).

**Claude Code:**

```powershell
claude mcp add rust-devices -- C:\Tools\rust-devices-mcp\rust-devices-mcp.exe mcp
```

**OpenCode:** same command + `["mcp"]` in your MCP server config.

**Manual probe** (no client needed):

```powershell
'{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | .\rust-devices-mcp.exe mcp
```

## 4. Troubleshooting

| Symptom | Cause / fix |
|---|---|
| SmartScreen block | Unsigned v0.1 binary — *More info → Run anyway* |
| Client shows no tools | Config path typo, or Claude Desktop wasn't fully restarted |
| Garbage on stdout | Nothing should log to stdout; report it as a bug (stdio invariant) |
| Real plug readings | Not in v0.1 — stub warning above; tier-2 delivers hardware truth |

## 5. Uninstall / upgrade

Delete the folder. For a new release, overwrite the exes in place — client
configs keep working because the path doesn't change.
