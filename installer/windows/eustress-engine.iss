; =============================================================================
; Eustress Engine - Windows Installer Script (Inno Setup)
; =============================================================================
; Build: iscc eustress-engine.iss
; Output: EustressEngine-Setup.exe
; =============================================================================

#define MyAppName "Eustress Engine"
#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif
#define MyAppPublisher "Eustress"
#define MyAppURL "https://eustress.dev"
#define MyAppExeName "eustress-engine.exe"

[Setup]
AppId={{A1B2C3D4-E5F6-7890-ABCD-EF1234567890}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}/download
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
AllowNoIcons=yes
SetupIconFile=..\..\eustress\crates\engine\assets\icon.ico
; dist\windows matches the CI Package step's existing output convention
; (release.yml already `mkdir -p dist` for the zip artifact). NOT
; downloads\windows — that path mirrors the LIVE downloads.eustress.dev
; R2 bucket contents (a different, already-in-use release channel) and
; writing local build output there would be confusing at best.
OutputDir=..\..\dist\windows
OutputBaseFilename=EustressEngine-Setup
; Installer settings
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
; Require admin for Program Files
PrivilegesRequired=admin
; Minimum Windows version (Windows 10)
MinVersion=10.0
; Architecture
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Uninstaller
UninstallDisplayIcon={app}\{#MyAppExeName}
UninstallDisplayName={#MyAppName}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
; Main executable
Source: "..\..\eustress\target\release\eustress-engine.exe"; DestDir: "{app}"; Flags: ignoreversion

; Rune LSP server — ships alongside the engine so external IDEs (Windsurf,
; VS Code, Cursor) get Rune intelligence without a second download. The
; engine launches this binary on startup from the install directory.
; `skipifsourcedoesntexist`: it IS a [[bin]] in the eustress-engine package
; gated by the `lsp` feature (on by default via `core`), so plain
; `cargo build --release --package eustress-engine` should already produce
; it alongside eustress-engine.exe — but the CI Windows job has never been
; verified to package it, so this stays a safety net rather than a hard
; requirement until that's confirmed.
Source: "..\..\eustress\target\release\eustress-lsp.exe"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist

; MCP server — exposes the Universe (Spaces, scripts, entities, assets,
; conversations) to any MCP-compatible AI client (Windsurf, Cursor,
; Claude Desktop). Lives in a SEPARATE package (eustress-mcp-server, bin
; eustress-mcp) that the CI Windows job's `--package eustress-engine`
; build does NOT produce today — `skipifsourcedoesntexist` keeps this
; installer buildable now; wire a build step for it into release.yml
; separately when the MCP binary is meant to ship in the installer.
Source: "..\..\eustress\target\release\eustress-mcp.exe"; DestDir: "{app}"; Flags: ignoreversion skipifsourcedoesntexist

; Assets folder — shaders, monaco, parts, lighting_templates, icons,
; characters. Excludes the Linux-only install-script subtree.
Source: "..\..\eustress\crates\engine\assets\*"; DestDir: "{app}\assets"; Excludes: "\linux\*"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[Registry]
; File association for .eustress files
Root: HKCR; Subkey: ".eustress"; ValueType: string; ValueName: ""; ValueData: "EustressProject"; Flags: uninsdeletevalue
Root: HKCR; Subkey: "EustressProject"; ValueType: string; ValueName: ""; ValueData: "Eustress Project"; Flags: uninsdeletekey
Root: HKCR; Subkey: "EustressProject\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName},0"
Root: HKCR; Subkey: "EustressProject\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""

; URL protocol handler for eustress://
Root: HKCR; Subkey: "eustress"; ValueType: string; ValueName: ""; ValueData: "URL:Eustress Protocol"; Flags: uninsdeletekey
Root: HKCR; Subkey: "eustress"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKCR; Subkey: "eustress\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName},0"
Root: HKCR; Subkey: "eustress\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""
