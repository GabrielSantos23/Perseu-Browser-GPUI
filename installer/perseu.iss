; Inno Setup 6 script. Build with installer\build.ps1, which compiles the
; release binary first. Per-user install, no administrator rights needed.

#define AppName "perseu"
#define AppExe AppName + ".exe"
#define SourceExe "..\target\release\" + AppExe
#define AppVersion GetStringFileInfo(SourceExe, "ProductVersion")
#define ProgId AppName + "HTML"
#define ClientKey "Software\Clients\StartMenuInternet\" + AppName
#define WebView2ClientKey "Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"

[Setup]
AppId={#AppName}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
OutputDir=..\dist
OutputBaseFilename={#AppName}-setup-{#AppVersion}
SetupIconFile=..\crates\app\assets\icon.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
#ifdef Sign
SignTool=signtool
SignedUninstaller=yes
#endif

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

; Mirrors crates/app/src/default_browser.rs so Windows lists the app under Default apps.
[Registry]
Root: HKCU; Subkey: "{#ClientKey}"; ValueType: string; ValueData: "{#AppName}"; Flags: uninsdeletekey
Root: HKCU; Subkey: "{#ClientKey}\DefaultIcon"; ValueType: string; ValueData: """{app}\{#AppExe}"",0"
Root: HKCU; Subkey: "{#ClientKey}\shell\open\command"; ValueType: string; ValueData: """{app}\{#AppExe}"""
Root: HKCU; Subkey: "{#ClientKey}\Capabilities"; ValueType: string; ValueName: "ApplicationName"; ValueData: "{#AppName}"
Root: HKCU; Subkey: "{#ClientKey}\Capabilities"; ValueType: string; ValueName: "ApplicationDescription"; ValueData: "A sidebar-first web browser"
Root: HKCU; Subkey: "{#ClientKey}\Capabilities"; ValueType: string; ValueName: "ApplicationIcon"; ValueData: """{app}\{#AppExe}"",0"
Root: HKCU; Subkey: "{#ClientKey}\Capabilities\URLAssociations"; ValueType: string; ValueName: "http"; ValueData: "{#ProgId}"
Root: HKCU; Subkey: "{#ClientKey}\Capabilities\URLAssociations"; ValueType: string; ValueName: "https"; ValueData: "{#ProgId}"
Root: HKCU; Subkey: "{#ClientKey}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".htm"; ValueData: "{#ProgId}"
Root: HKCU; Subkey: "{#ClientKey}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".html"; ValueData: "{#ProgId}"
Root: HKCU; Subkey: "Software\RegisteredApplications"; ValueType: string; ValueName: "{#AppName}"; ValueData: "{#ClientKey}\Capabilities"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\{#ProgId}"; ValueType: string; ValueData: "{#AppName} Document"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\{#ProgId}\DefaultIcon"; ValueType: string; ValueData: """{app}\{#AppExe}"",0"
Root: HKCU; Subkey: "Software\Classes\{#ProgId}\shell\open\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" ""%1"""

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
; An update from the app runs silently and opens the app again when done,
; unless it ran because the app was closing (/NORELAUNCH).
Filename: "{app}\{#AppExe}"; Flags: nowait; Check: WizardSilent and not NoRelaunch

[Code]
function NoRelaunch(): Boolean;
begin
  Result := Pos('/NORELAUNCH', UpperCase(GetCmdTail())) > 0;
end;

function WebView2Installed(): Boolean;
var
  Version: String;
begin
  Result :=
    (RegQueryStringValue(HKLM, 'SOFTWARE\WOW6432Node\{#WebView2ClientKey}', 'pv', Version) and (Version <> '') and (Version <> '0.0.0.0')) or
    (RegQueryStringValue(HKCU, 'Software\{#WebView2ClientKey}', 'pv', Version) and (Version <> '') and (Version <> '0.0.0.0'));
end;

function InitializeSetup(): Boolean;
begin
  Result := True;
  if not WebView2Installed() then
    Result := MsgBox('{#AppName} needs the Microsoft Edge WebView2 Runtime, which was not found.' + #13#10#13#10 +
      'Windows 11 includes it. On Windows 10, install it from https://go.microsoft.com/fwlink/p/?LinkId=2124703 and run this setup again.' + #13#10#13#10 +
      'Install anyway?', mbConfirmation, MB_YESNO) = IDYES;
end;
