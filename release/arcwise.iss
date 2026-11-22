; Builds the Arcwise installer. release/package.sh runs it from the repository's root:
;   iscc /DAppVersion=0.11.0 /DStaging=<folder> /DOutputDir=<folder> release\arcwise.iss
; Staging holds the files it installs. It installs for the current user only, so it never asks
; for administrator rights.

#ifndef AppVersion
  #error Pass the version, such as /DAppVersion=0.11.0
#endif
#ifndef Staging
  #error Pass the folder of files to install, such as /DStaging=C:\staging
#endif
#ifndef OutputDir
  #define OutputDir "..\dist"
#endif
; Windows version resources take numbers only, so drop any pre-release suffix such as -beta.1.
#define NumericVersion Copy(AppVersion, 1, Pos("-", AppVersion + "-") - 1)
; Created by the app while it runs: RUNNING_MUTEX in crates/arcwise_ffi/src/running.rs.
#define RunningMutex "Arcwise.Running"

[Setup]
; Never change AppId: Windows uses it to find an installed Arcwise when a newer version installs
; over it.
AppId={{C882D75D-CDF1-4F35-91C7-8D8A7A01A810}
AppName=Arcwise
AppVersion={#AppVersion}
AppVerName=Arcwise {#AppVersion}
AppPublisher=265866
AppPublisherURL=https://github.com/265866/arcwise
AppSupportURL=https://github.com/265866/arcwise/issues
AppUpdatesURL=https://github.com/265866/arcwise/releases
VersionInfoVersion={#NumericVersion}
VersionInfoProductVersion={#NumericVersion}
VersionInfoProductTextVersion={#AppVersion}
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
DefaultDirName={autopf}\Arcwise
DisableProgramGroupPage=yes
DisableDirPage=auto
UninstallDisplayName=Arcwise
UninstallDisplayIcon={app}\arcwise.exe
SetupIconFile=..\crates\arcwise\assets\arcwise.ico
OutputDir={#OutputDir}
OutputBaseFilename=arcwise-{#AppVersion}-x64-setup
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
; Closes a running Arcwise before replacing its files. Silent installs, such as the ones the app
; starts to update itself, close it without asking.
CloseApplications=yes
RestartApplications=no
; Two copies of Arcwise can each finish downloading an update and start its installer.
SetupMutex=Arcwise.Setup

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[InstallDelete]
; Versions up to 0.10.0 were MIT-licensed alone and installed this; the license files below
; replace it.
Type: files; Name: "{app}\LICENSE.txt"

[Files]
Source: "{#Staging}\arcwise.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Staging}\THIRD-PARTY-LICENSES.html"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Staging}\LICENSE-MIT.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Staging}\LICENSE-APACHE.txt"; DestDir: "{app}"; Flags: ignoreversion

[UninstallDelete]
; Installers the app downloaded to update itself: directory() in crates/arcwise_update.
Type: filesandordirs; Name: "{localappdata}\Arcwise\updates"
Type: dirifempty; Name: "{localappdata}\Arcwise"

[Icons]
Name: "{autoprograms}\Arcwise"; Filename: "{app}\arcwise.exe"
Name: "{autodesktop}\Arcwise"; Filename: "{app}\arcwise.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\arcwise.exe"; Description: "{cm:LaunchProgram,Arcwise}"; Flags: nowait postinstall skipifsilent
; The app's updater installs silently with /RELAUNCH to open the new version afterwards.
Filename: "{app}\arcwise.exe"; Parameters: "--resume"; Flags: nowait; Check: WizardSilent and RelaunchRequested

[Code]
function RelaunchRequested: Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), '/RELAUNCH') = 0 then
      Result := True;
end;

// The uninstaller cannot remove arcwise.exe while it runs, so it asks for Arcwise to be closed
// first. Setup does not need this: CloseApplications closes the app for it.
function InitializeUninstall: Boolean;
begin
  Result := True;
  while Result and CheckForMutexes('{#RunningMutex}') do
  begin
    if UninstallSilent then
      Result := False
    else
      Result := MsgBox('Arcwise is running. Close it, then click OK to continue uninstalling.',
        mbError, MB_OKCANCEL) = IDOK;
  end;
end;

// Settings stay by default, so reinstalling picks up where Arcwise left off.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if (CurUninstallStep = usPostUninstall) and not UninstallSilent then
    if MsgBox('Also delete your Arcwise settings?', mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
      DelTree(ExpandConstant('{userappdata}\Arcwise'), True, True, True);
end;
