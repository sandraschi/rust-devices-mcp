; Devices (Rust) v0.2.0 — current-user installer (no admin, no UAC).
; Built with NSIS 3 (makensis installer/installer.nsi, CWD = repo root).
; Installs the egui dashboard + the CLI/MCP server side by side.

!include "MUI2.nsh"

Name "Devices (Rust) 0.2.0"
OutFile "..\target\release\bundle\Devices-Rust-0.2.0-setup.exe"
InstallDir "$LOCALAPPDATA\Programs\rust-devices-mcp"
RequestExecutionLevel user

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Section "Binaries" SecBinaries
  SetOutPath "$INSTDIR"
  File "..\target\release\devices-app.exe"
  File "..\target\release\rust-devices-mcp.exe"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\rust-devices-mcp" \
    "DisplayName" "Devices (Rust) 0.2.0"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\rust-devices-mcp" \
    "UninstallString" "$INSTDIR\uninstall.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\rust-devices-mcp" \
    "DisplayIcon" "$INSTDIR\devices-app.exe"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\rust-devices-mcp" \
    "DisplayVersion" "0.2.0"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\rust-devices-mcp" \
    "Publisher" "sandraschi"
SectionEnd

Section "Start Menu shortcuts" SecShortcuts
  CreateDirectory "$SMPROGRAMS\Devices (Rust)"
  CreateShortcut "$SMPROGRAMS\Devices (Rust)\Devices (Rust).lnk" "$INSTDIR\devices-app.exe"
  CreateShortcut "$SMPROGRAMS\Devices (Rust)\Uninstall.lnk" "$INSTDIR\uninstall.exe"
SectionEnd

Section "uninstall"
  Delete "$INSTDIR\devices-app.exe"
  Delete "$INSTDIR\rust-devices-mcp.exe"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  Delete "$SMPROGRAMS\Devices (Rust)\Devices (Rust).lnk"
  Delete "$SMPROGRAMS\Devices (Rust)\Uninstall.lnk"
  RMDir "$SMPROGRAMS\Devices (Rust)"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\rust-devices-mcp"
SectionEnd
