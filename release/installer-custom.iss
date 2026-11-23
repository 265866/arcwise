; Arcwise's part of its installer: what release/arcwise.iss had that trellis's
; release/installer.iss doesn't write.

; Created by the app while it runs: RUNNING_MUTEX in crates/arcwise_ffi/src/running.rs.
#define RunningMutex "Arcwise.Running"
; The mutex arcwise.iss gave Setup before trellis wrote the installer.
#define SetupMutex "Arcwise.Setup"

[Setup]
SetupIconFile=..\crates\arcwise\assets\arcwise.ico

[InstallDelete]
; Versions up to 0.10.0 were MIT-licensed alone and installed this; the license files below
; replace it.
Type: files; Name: "{app}\LICENSE.txt"

[UninstallDelete]
; Installers the app downloaded to update itself: directory() in crates/arcwise_update.
Type: filesandordirs; Name: "{localappdata}\Arcwise\updates"
Type: dirifempty; Name: "{localappdata}\Arcwise"

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
