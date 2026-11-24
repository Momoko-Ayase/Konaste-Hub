; Konaste Hub native (GPUI) installer.
;
; Build through scripts/build-installer.ps1; it supplies AppVersion, SourceExe,
; OutputDir, RepoRoot, and LicenseFile. Inno Setup 6.3 or newer is required.
;
; This is a per-user install. The Hub does not need administrator rights to
; run, and it writes its own state under %LOCALAPPDATA%\konaste-hub.
; It never bundles game files or KONAMI components.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

; Inno Setup wants four numeric components in VersionInfoVersion. The build
; script passes this separately; the default pads the workspace version.
#ifndef VersionInfoVersion
  #define VersionInfoVersion AppVersion + ".0"
#endif

; Defaults matter only when compiling from this folder without /D arguments.
#ifndef RepoRoot
  #define RepoRoot SourcePath + ".."
#endif

#ifndef SourceExe
  #define SourceExe RepoRoot + "\target\release\konaste-hub.exe"
#endif

#ifndef OutputDir
  #define OutputDir RepoRoot + "\dist"
#endif

#ifndef LicenseFile
  #define LicenseFile RepoRoot + "\LICENSE"
#endif

#define VCRedistPath RepoRoot + "\installer\redist\vc_redist.x64.exe"
#define WebView2InstallerPath RepoRoot + "\installer\redist\MicrosoftEdgeWebView2RuntimeInstallerX64.exe"

; Optional local copies. The build script never downloads either file.
#if FileExists(VCRedistPath)
  #define HaveVCRedist
#endif
#if FileExists(WebView2InstallerPath)
  #define HaveWebView2Installer
#endif

[Setup]
AppId={{5F08EC78-9C91-42EE-9279-D29BB24212E9}
AppName=Konaste Hub
AppVersion={#AppVersion}
AppVerName=Konaste Hub {#AppVersion}
AppPublisher=Konaste Hub contributors
AppCopyright=Copyright (C) 2026 Konaste Hub contributors
VersionInfoVersion={#VersionInfoVersion}
VersionInfoProductName=Konaste Hub
VersionInfoDescription=Konaste Hub Setup
DefaultDirName={autopf}\Konaste Hub
DefaultGroupName=Konaste Hub
DisableProgramGroupPage=yes
; These directives take the unquoted rest of the line, so spaces in the repo
; path are fine. Only [Files] Source values quote their path.
LicenseFile={#LicenseFile}
OutputDir={#OutputDir}
OutputBaseFilename=KonasteHub-{#AppVersion}-setup
SetupIconFile={#RepoRoot}\crates\konaste-hub\assets\icon.ico
UninstallDisplayIcon={app}\konaste-hub.exe
UninstallDisplayName=Konaste Hub
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
PrivilegesRequired=lowest
; Non-administrative mode is 32-bit Setup. Do not set
; ArchitecturesInstallIn64BitMode: Inno Setup does not allow it here, and the
; Hub needs no HKLM writes. x64os (not x64compatible) keeps ARM64 out, where
; the x64 runtime check and the x64-only binary would need separate handling.
ArchitecturesAllowed=x64os
MinVersion=10.0
ChangesAssociations=no
; Every Hub process (UI, tray-only, headless, runtime worker) holds the named
; mutex KonasteHub (platform::APP_MUTEX). [Code] checks it instead of the
; AppMutex directive, so a self-update can wait for the Hub to exit.
CloseApplications=no
RestartApplications=no
AllowNoIcons=yes
UsePreviousAppDir=yes
DirExistsWarning=auto

; Setup UI only. The Hub itself ships five languages. Inno Setup bundles
; English and Japanese; other translations would add a file we do not vendor.
[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

; The uninstaller removes {app}, then asks whether to also delete the data
; folder (see CurUninstallStepChanged). Game installations always stay.
[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "konaste-hub.exe"; Flags: ignoreversion
Source: "{#RepoRoot}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepoRoot}\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion
#ifdef HaveVCRedist
Source: "{#VCRedistPath}"; DestDir: "{tmp}"; Flags: dontcopy
#endif
#ifdef HaveWebView2Installer
Source: "{#WebView2InstallerPath}"; DestDir: "{tmp}"; Flags: dontcopy
#endif

[Icons]
Name: "{autoprograms}\Konaste Hub"; Filename: "{app}\konaste-hub.exe"
Name: "{autodesktop}\Konaste Hub"; Filename: "{app}\konaste-hub.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\konaste-hub.exe"; Description: "{cm:LaunchProgram,Konaste Hub}"; Flags: nowait postinstall skipifsilent
; A self-update runs silently and starts the Hub again when it finishes.
Filename: "{app}\konaste-hub.exe"; Flags: nowait; Check: IsSelfUpdate

[Code]
const
  HubMutex = 'KonasteHub';

var
  PrereqChecked: Boolean;

// The Hub starts Setup with /RESTARTHUB when the user accepts an update
// (crates/konaste-hub/src/self_update.rs).
function IsSelfUpdate: Boolean;
begin
  Result := Pos('/RESTARTHUB', Uppercase(GetCmdTail)) > 0;
end;

// True once no Hub process holds the mutex. A self-update gives the Hub that
// started it time to exit; a normal run checks once.
function HubClosed(WaitSeconds: Integer): Boolean;
var
  Waited: Integer;
begin
  Waited := 0;
  while CheckForMutexes(HubMutex) and (Waited < WaitSeconds * 10) do
  begin
    Sleep(100);
    Waited := Waited + 1;
  end;
  Result := not CheckForMutexes(HubMutex);
end;

function InitializeSetup: Boolean;
var
  Wait: Integer;
begin
  Wait := 0;
  if IsSelfUpdate then
    Wait := 30;
  Result := HubClosed(Wait);
  if not Result then
    SuppressibleMsgBox('Konaste Hub is running. Close it, including from the notification area, and run Setup again.',
      mbInformation, MB_OK, IDOK);
end;

// A Hub started during the wizard is caught here, before any file is replaced.
//
// Setup writes only {app}. It never writes to the data folder or to a game
// installation.
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  NeedsRestart := False;
  if not HubClosed(0) then
    Result := 'Konaste Hub is running. Close it, including from the notification area, and run Setup again.';
end;

// The release binary is x64 and imports VCRUNTIME140.dll and
// VCRUNTIME140_1.dll. These ship with the Microsoft Visual C++ 2015-2022
// Redistributable, not with Windows itself.
//
// PrivilegesRequired=lowest makes this a 32-bit Setup process, so {sys} is
// SysWOW64. The x64 runtime lives in the native System32, which is
// {sysnative} here. The registry value is a second signal.
function VCRuntimePresent: Boolean;
var
  Version: String;
begin
  Result := FileExists(ExpandConstant('{sysnative}\VCRUNTIME140.dll')) and
            FileExists(ExpandConstant('{sysnative}\VCRUNTIME140_1.dll'));
  if (not Result) and
     RegQueryStringValue(HKLM64, 'SOFTWARE\Microsoft\VisualStudio\14.0\VC\RUNTIME\X64', 'Version', Version) then
    Result := Version <> '';
end;

// Evergreen WebView2 Runtime. Windows 11 normally includes it; Windows 10
// does not. The Hub's WebView2Loader is linked statically, so only the
// runtime is needed. Both machine-wide and per-user installs are checked.
function WebView2Version: String;
var
  Value: String;
begin
  Result := '';
  if RegQueryStringValue(HKCU, 'Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}', 'pv', Value) then
    Result := Value;
  if (Result = '') and
     RegQueryStringValue(HKLM32, 'SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}', 'pv', Value) then
    Result := Value;
  if (Result = '') and
     RegQueryStringValue(HKLM64, 'SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}', 'pv', Value) then
    Result := Value;
end;

function WebView2Present: Boolean;
begin
  // Evergreen installs machine-wide under Program Files (x86), or per-user
  // under LOCALAPPDATA when the machine is not writable.
  Result := (WebView2Version <> '') or
            DirExists(ExpandConstant('{commonpf32}\Microsoft\EdgeWebView\Application')) or
            DirExists(ExpandConstant('{localappdata}\Microsoft\EdgeWebView\Application'));
end;

procedure RunBundled(const FileName, Parameters: String);
var
  ResultCode: Integer;
begin
  ExtractTemporaryFile(FileName);
  if not Exec(ExpandConstant('{tmp}\' + FileName), Parameters, '', SW_SHOW, ewWaitUntilTerminated, ResultCode) then
    SuppressibleMsgBox('Could not start ' + FileName + '.', mbError, MB_OK, IDOK)
  else if (ResultCode <> 0) and (ResultCode <> 3010) then
    SuppressibleMsgBox(FileName + ' exited with code ' + IntToStr(ResultCode) + '.', mbError, MB_OK, IDOK);
end;

procedure CheckPrerequisites;
var
  Answer: Integer;
  ErrorCode: Integer;
begin
  if not VCRuntimePresent then
  begin
#ifdef HaveVCRedist
    Answer := SuppressibleMsgBox('Microsoft Visual C++ 2015-2022 Redistributable (x64) was not detected.' + #13#10 + #13#10 +
      'Konaste Hub needs it to start. Install the bundled copy now?',
      mbConfirmation, MB_YESNO, IDNO);
    if Answer = IDYES then
      RunBundled('vc_redist.x64.exe', '/install /quiet /norestart');
#else
    Answer := SuppressibleMsgBox('Microsoft Visual C++ 2015-2022 Redistributable (x64) was not detected.' + #13#10 + #13#10 +
      'Setup can continue, but Konaste Hub will not start until it is installed.' + #13#10 +
      'Open the Microsoft download page now?',
      mbConfirmation, MB_YESNO, IDNO);
    if Answer = IDYES then
      ShellExec('open', 'https://learn.microsoft.com/cpp/windows/latest-supported-vc-redist',
        '', '', SW_SHOWNORMAL, ewNoWait, ErrorCode);
#endif
  end;

  if not WebView2Present then
  begin
#ifdef HaveWebView2Installer
    Answer := SuppressibleMsgBox('The Microsoft Edge WebView2 Runtime was not detected.' + #13#10 + #13#10 +
      'Konaste Hub uses it for official sign-in. Install the bundled runtime now?',
      mbConfirmation, MB_YESNO, IDNO);
    if Answer = IDYES then
      RunBundled('MicrosoftEdgeWebView2RuntimeInstallerX64.exe', '/silent /install');
#else
    Answer := SuppressibleMsgBox('The Microsoft Edge WebView2 Runtime was not detected.' + #13#10 + #13#10 +
      'Konaste Hub uses it for official sign-in. Setup can continue, but sign-in will not work until it is installed.' + #13#10 +
      'Open the Microsoft WebView2 page now?',
      mbConfirmation, MB_YESNO, IDNO);
    if Answer = IDYES then
      ShellExec('open', 'https://developer.microsoft.com/microsoft-edge/webview2/',
        '', '', SW_SHOWNORMAL, ewNoWait, ErrorCode);
#endif
  end;
end;

// Uninstall: the Hub must be closed, then the user chooses whether to delete
// the data folder (settings, saved sign-in, runtime cache, logs). It defaults
// to No and is skipped in silent mode. Games live in their own folders and are
// never removed.
function InitializeUninstall(): Boolean;
begin
  Result := not CheckForMutexes(HubMutex);
  if not Result then
    SuppressibleMsgBox('Konaste Hub is running. Close it, including from the notification area, and run the uninstaller again.',
      mbInformation, MB_OK, IDOK);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Data: String;
begin
  if CurUninstallStep <> usPostUninstall then
    Exit;
  Data := ExpandConstant('{localappdata}\konaste-hub');
  if not DirExists(Data) or UninstallSilent or CheckForMutexes(HubMutex) then
    Exit;
  if MsgBox('Also delete Konaste Hub settings, saved sign-in, and cache?' + #13#10 + #13#10 +
    Data + #13#10 + #13#10 + 'Installed games are not affected.',
    mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
    DelTree(Data, True, True, True);
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if (CurPageID = wpReady) and (not PrereqChecked) then
  begin
    PrereqChecked := True;
    CheckPrerequisites;
  end;
end;
