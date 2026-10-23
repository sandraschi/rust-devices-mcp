@echo off
setlocal
REM Double-click launcher: delegates to start.ps1 (keeps window open on error).
set "HERE=%~dp0"
where pwsh.exe >nul 2>&1
if %errorlevel%==0 (
  pwsh.exe -NoProfile -ExecutionPolicy Bypass -File "%HERE%start.ps1" %*
) else (
  powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%HERE%start.ps1" %*
)
if errorlevel 1 (
  echo.
  echo [rust-devices-mcp-start] exited with error - window held open, instacrash guard
  pause
  exit /b 1
)
endlocal
exit /b 0
