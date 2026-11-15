; Inno Setup script for the standalone Askeri Ruler installer.
; Built by installer\build-installer.ps1, which passes:
;   /DAppVersion=x.y.z  /DSourceExe=...\askeri-ruller.exe  /DIconFile=...\app.ico  /DOutputDir=...

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

#define AppName "Askeri Ruler"
#define AppExe "askeri-ruller.exe"
; Must match VALUE_NAME and AUTOSTART_ARG in src\autostart.rs.
#define RunValue "AskeriRuller"
#define AutostartArg "--autostart"

[Setup]
; Never change AppId: it identifies the app for upgrades and uninstall.
AppId={{6F0B3C2A-8E4D-4C1B-9A57-3D2E8B7C5A10}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Maria Askeri
AppPublisherURL=https://github.com/mashoolity/askeri-ruler
AppSupportURL=https://github.com/mashoolity/askeri-ruler/issues
AppUpdatesURL=https://github.com/mashoolity/askeri-ruler/releases
VersionInfoVersion={#AppVersion}
; Per-user install: no administrator prompt.
PrivilegesRequired=lowest
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=auto
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
SetupIconFile={#IconFile}
OutputDir={#OutputDir}
OutputBaseFilename=AskeriRuler-Setup-{#AppVersion}
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "autostart"; Description: "Start {#AppName} when I sign in to Windows"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"

[Registry]
; Same entry the app's own "Start with Windows" setting writes.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; \
  ValueName: "{#RunValue}"; ValueData: """{app}\{#AppExe}"" {#AutostartArg}"; Tasks: autostart

[Run]
Filename: "{app}\{#AppExe}"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent

[Code]
// Ask a running copy to exit (taskkill without /f sends WM_CLOSE, which the
// app handles by quitting cleanly and removing its tray icon).
procedure CloseRunningApp();
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/im {#AppExe}', '', SW_HIDE,
    ewWaitUntilTerminated, ResultCode);
  Sleep(800);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  CloseRunningApp();
  Result := '';
end;

function InitializeUninstall(): Boolean;
begin
  CloseRunningApp();
  Result := True;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  // The user may have enabled "Start with Windows" from the app's settings
  // rather than the installer task, so always remove the entry.
  if CurUninstallStep = usPostUninstall then
    RegDeleteValue(HKEY_CURRENT_USER, 'Software\Microsoft\Windows\CurrentVersion\Run', '{#RunValue}');
end;
