; =============================================================================
; Eustress Engine - Windows Installer Script (Inno Setup)
; =============================================================================
; Stage first, then compile:
;   pwsh installer/windows/stage.ps1 -BinDir <dir holding eustress-engine.exe>
;   iscc /DMyAppVersion=X.Y.Z installer/windows/eustress-engine.iss
; Output: dist\windows\EustressEngine-Setup.exe
; =============================================================================

#define MyAppName "Eustress Engine"
#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif
; The layout stage.ps1 builds, which the release zip also ships.
#ifndef StageDir
  #define StageDir "..\..\dist\windows\stage"
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
; Everything stage.ps1 staged: eustress-engine.exe, eustress-lsp.exe (the
; Rune language server the editor starts from this directory), assets\,
; common\assets\ and docs\. The engine finds each one beside its exe.
;
; Not shipped yet: eustress-mcp.exe, the MCP server for external AI clients,
; lives in its own package (eustress-mcp-server) and is not built here.
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

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
