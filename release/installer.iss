; The Windows installer, which trellis writes from the `installer` entry of the `release` answer in
; .copier-answers.yml and rewrites on every update. Put the project's own additions in
; installer-custom.iss, which this script includes before its own sections, never here. Inno Setup
; refuses a [Setup] directive set twice, so the custom file changes the two this script lets it
; with a #define: CloseApplications and SetupMutex. The release build compiles it
; with Inno Setup for each target the entry names:
;   ISCC /DTarget=<target triple> /DAppVersion=<version> /DStaging=<folder> /O<folder> release\installer.iss
; Staging holds the files release/package.sh staged to install. docs/maintaining.md says what it
; gets.

#ifndef Target
  #error Pass the target triple, such as /DTarget=x86_64-pc-windows-msvc
#endif
#ifndef AppVersion
  #error Pass the version, such as /DAppVersion=1.4.2
#endif
#ifndef Staging
  #error Pass the folder of files to install, such as /DStaging=C:\staging
#endif

#if Target == "x86_64-pc-windows-msvc"
  ; Windows finds the installed program by its AppId, so it never changes.
  #define AppId "C882D75D-CDF1-4F35-91C7-8D8A7A01A810"
  #define AppName "Arcwise"
  #define Architecture "x64compatible"
  #define OutputName "arcwise-" + AppVersion + "-x64-setup"
#else
  #error The release answer's installer entry doesn't name this target
#endif
#define AppExe "arcwise.exe"
; Windows version resources take numbers only, so they drop a pre-release suffix such as -beta.1
; and build metadata such as +linux.
#define Release Copy(AppVersion, 1, Pos("-", AppVersion + "-") - 1)
#define NumericVersion Copy(Release, 1, Pos("+", Release + "+") - 1)

#include "installer-custom.iss"

; Closes the running program before it replaces its files, without asking in a silent install,
; such as the one a program starts to update itself. A program with no window can't be closed
; this way: setup then rolls back and exits with 5, unless this is "force", as
; docs/maintaining.md says.
#ifndef CloseApplications
  #define CloseApplications "yes"
#endif
; Two copies of the program can each download an update and start its installer.
#ifndef SetupMutex
  #define SetupMutex AppId + ".Setup"
#endif


[Setup]
AppId={{{#AppId}}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=265866
AppPublisherURL=https://github.com/265866/arcwise
AppSupportURL=https://github.com/265866/arcwise/issues
AppUpdatesURL=https://github.com/265866/arcwise/releases
VersionInfoVersion={#NumericVersion}
VersionInfoProductVersion={#NumericVersion}
VersionInfoProductTextVersion={#AppVersion}
; Installs for the current user alone, under %LOCALAPPDATA%\Programs, so it never asks for
; administrator rights.
PrivilegesRequired=lowest
ArchitecturesAllowed={#Architecture}
ArchitecturesInstallIn64BitMode={#Architecture}
MinVersion=10.0
DefaultDirName={autopf}\{#AppName}
DisableDirPage=auto
DisableProgramGroupPage=yes
UninstallDisplayName={#AppName}
UninstallDisplayIcon={app}\{#AppExe}
OutputBaseFilename={#OutputName}
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
CloseApplications={#CloseApplications}
RestartApplications=no
SetupMutex={#SetupMutex}
; Signing: the release ships the installer unsigned. Signing it would set SignTool and
; SignedUninstaller=yes in installer-custom.iss, since an update rewrites this file, and the
; release build's ISCC call would need a trellis change to pass the sign tool with /S.

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#Staging}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon
