#Requires -Version 5.1
<#
.SYNOPSIS
  Dev launcher: L3 egui app (release exe preferred, else cargo run).
.DESCRIPTION
  Double-click QoL (see start.bat). Prefers target\release\<app>.exe,
  falls back to `cargo run -p <crate>`. Extra args pass through to the app.
  Pure ASCII on purpose: WinPS 5.1 misparses BOM-less UTF-8 (see BUG-087).
#>
$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $MyInvocation.MyCommand.Path
$AppCrate = "devices-app"
$AppBin = "devices-app"
$releaseExe = Join-Path $RepoRoot "target\release\$AppBin.exe"
if (Test-Path $releaseExe) {
  & $releaseExe @args
  exit $LASTEXITCODE
}
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
Set-Location $RepoRoot
& "$env:USERPROFILE\.cargo\bin\cargo.exe" run -p $AppCrate -- @args
exit $LASTEXITCODE
