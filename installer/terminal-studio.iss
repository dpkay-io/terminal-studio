; Terminal Studio — Inno Setup Installer Script
; Build: iscc /DAppVersion=X.Y.Z terminal-studio.iss

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

#define AppName "Terminal Studio"
#define AppExeName "terminal-studio.exe"
#define AppPublisher "dpkay-io"
#define AppPublisherURL "https://github.com/dpkay-io/terminal-studio"
#define AppSupportURL "https://github.com/dpkay-io/terminal-studio/issues"

[Setup]
AppId={{A1B2C3D4-E5F6-7890-ABCD-EF1234567890}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppPublisherURL}
AppSupportURL={#AppSupportURL}
DefaultDirName={autopf}\Terminal Studio
DefaultGroupName={#AppName}
LicenseFile=..\LICENSE
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\{#AppExeName}
OutputDir=Output
OutputBaseFilename=terminal-studio-setup
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked
Name: "addtopath"; Description: "Add to PATH (allows running from command line)"; GroupDescription: "System integration:"; Flags: unchecked

[Files]
Source: "..\target\x86_64-pc-windows-msvc\release\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExeName}"; WorkingDir: "{userprofile}"
Name: "{group}\Uninstall {#AppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon; WorkingDir: "{userprofile}"

[Run]
Filename: "{app}\{#AppExeName}"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent

[Code]
const
  SMTO_ABORTIFHUNG = 2;
  WM_SETTINGCHANGE = $001A;

function SendMessageTimeoutW(hWnd: LongInt; Msg: Cardinal; wParam: Cardinal; lParam: String; fuFlags: Cardinal; uTimeout: Cardinal; var lpdwResult: Cardinal): Cardinal;
  external 'SendMessageTimeoutW@user32.dll stdcall';

procedure BroadcastEnvironmentChange();
var
  Dummy: Cardinal;
begin
  SendMessageTimeoutW(HWND_BROADCAST, WM_SETTINGCHANGE, 0,
    'Environment', SMTO_ABORTIFHUNG, 5000, Dummy);
end;

function NeedsAddPath(Param: string): Boolean;
var
  OrigPath: string;
begin
  Result := True;
  if not RegQueryStringValue(HKEY_CURRENT_USER,
    'Environment', 'Path', OrigPath) then
    Exit;
  { trailing and leading semicolons for reliable substring match }
  OrigPath := ';' + OrigPath + ';';
  Param := ';' + Param + ';';
  if Pos(AnsiUppercase(Param), AnsiUppercase(OrigPath)) > 0 then
    Result := False;
end;

procedure AddToPath();
var
  OrigPath: string;
  AppDir: string;
begin
  AppDir := ExpandConstant('{app}');
  if not NeedsAddPath(AppDir) then
    Exit;
  if not RegQueryStringValue(HKEY_CURRENT_USER,
    'Environment', 'Path', OrigPath) then
    OrigPath := '';
  if (Length(OrigPath) > 0) and (OrigPath[Length(OrigPath)] <> ';') then
    OrigPath := OrigPath + ';';
  RegWriteExpandStringValue(HKEY_CURRENT_USER,
    'Environment', 'Path', OrigPath + AppDir);
end;

procedure RemoveFromPath();
var
  OrigPath: string;
  AppDir: string;
  Prefix: string;
  P: Integer;
begin
  AppDir := ExpandConstant('{app}');
  if not RegQueryStringValue(HKEY_CURRENT_USER,
    'Environment', 'Path', OrigPath) then
    Exit;
  { wrap both in semicolons for reliable boundary matching }
  Prefix := ';' + AnsiUppercase(OrigPath) + ';';
  P := Pos(';' + AnsiUppercase(AppDir) + ';', Prefix);
  if P = 0 then
    Exit;
  { P points into the prefixed string; subtract 1 to map back to OrigPath }
  { delete the app dir plus the leading semicolon that separates it }
  Delete(OrigPath, P, Length(AppDir) + 1);
  { clean up doubled semicolons left behind }
  StringChangeEx(OrigPath, ';;', ';', True);
  { strip leading/trailing semicolons }
  if (Length(OrigPath) > 0) and (OrigPath[1] = ';') then
    Delete(OrigPath, 1, 1);
  if (Length(OrigPath) > 0) and (OrigPath[Length(OrigPath)] = ';') then
    Delete(OrigPath, Length(OrigPath), 1);
  RegWriteExpandStringValue(HKEY_CURRENT_USER,
    'Environment', 'Path', OrigPath);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    if WizardIsTaskSelected('addtopath') then
    begin
      AddToPath();
      BroadcastEnvironmentChange();
    end;
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
  begin
    RemoveFromPath();
    BroadcastEnvironmentChange();
  end;
end;
