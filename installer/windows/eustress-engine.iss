; =============================================================================
; Eustress Engine - Windows Installer Script (Inno Setup)
; =============================================================================
; Build: iscc eustress-engine.iss
; Output: EustressEngine-Setup.exe
; =============================================================================

#define MyAppName "Eustress Engine"
#define MyAppVersion "0.1.0"
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
; Output to downloads folder
OutputDir=..\..\downloads\windows
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
Name: "quicklaunchicon"; Description: "{cm:CreateQuickLaunchIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked; OnlyBelowVersion: 6.1; Check: not IsAdminInstallMode

[Files]
; Main executable
Source: "..\..\eustress\target\release\eustress-engine.exe"; DestDir: "{app}"; Flags: ignoreversion

; Assets folder (if exists)
; Source: "..\..\eustress\assets\*"; DestDir: "{app}\assets"; Flags: ignoreversion recursesubdirs createallsubdirs; Check: DirExists(ExpandConstant('..\..\eustress\assets'))

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
