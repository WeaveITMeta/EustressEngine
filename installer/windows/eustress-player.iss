; =============================================================================
; Eustress Player - Windows Installer Script (Inno Setup)
; =============================================================================
; Stage first, then compile:
;   pwsh installer/stage-player.ps1 -BinDir <dir holding eustress-client.exe>
;   iscc /DMyAppVersion=X.Y.Z [/DSetupIcon=<icon.ico>] installer/windows/eustress-player.iss
; Output: dist\windows\EustressPlayer-Setup.exe
;
; SetupIcon is the icon.ico that eustress-client's build script renders from
; assets/icon.svg into its OUT_DIR; the release workflow passes it. Without it
; Setup carries Inno's default icon. The installed Player always shows its own
; icon, which is embedded in eustress-client.exe.
; =============================================================================

#define MyAppName "Eustress Player"
#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif
; The layout stage-player.ps1 builds, which the release zip also ships.
#ifndef StageDir
  #define StageDir "..\..\dist\player-stage"
#endif
#define MyAppPublisher "Eustress"
#define MyAppURL "https://eustress.dev"
#define MyAppExeName "eustress-client.exe"
; The Player's own link scheme. Windows gives each scheme to one application,
; and eustress:// belongs to Studio (eustress-engine.iss).
#define MyAppScheme "eustress-player"

[Setup]
; The Player's own identity. Sharing Studio's AppId would make each installer
; upgrade or uninstall the other.
AppId={{19C84FA9-8C4A-40A4-B020-D2A9FBE2B38B}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}/downloads/player
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
AllowNoIcons=yes
#ifdef SetupIcon
SetupIconFile={#SetupIcon}
#endif
OutputDir=..\..\dist\windows
OutputBaseFilename=EustressPlayer-Setup
; Installer settings
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
; Installs for the current user, with no administrator prompt: {autopf} is
; then %LOCALAPPDATA%\Programs and HKA is HKEY_CURRENT_USER. The setup dialog
; offers an install for all users instead, which moves both to the machine.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog commandline
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
; Everything stage-player.ps1 staged: eustress-client.exe and common\assets\,
; which the Player finds beside its exe.
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\{cm:UninstallProgram,{#MyAppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[Registry]
; URL protocol handler for eustress-player://. The browser hands the whole link
; to the Player as its one argument, and Launch::from_args reads it as the same
; link spelled with eustress://.
Root: HKA; Subkey: "Software\Classes\{#MyAppScheme}"; ValueType: string; ValueName: ""; ValueData: "URL:Eustress Player Protocol"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\{#MyAppScheme}"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\{#MyAppScheme}\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#MyAppExeName},0"
Root: HKA; Subkey: "Software\Classes\{#MyAppScheme}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExeName}"" ""%1"""
