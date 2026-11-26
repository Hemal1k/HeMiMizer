; HeMiMizer installer (Inno Setup 6), built by release.ps1:  ISCC /DAppVersion=<version> /DAppExe=<HeMiMizer.exe> /DOutputDir=<folder>
; For the current user or for all users - the installer asks (as the other HeMi apps). x64 only (it runs on Windows on
; ARM too, emulated). HeMiMizer itself always runs as administrator (its manifest), so it is started through the shell
; (shellexec), which asks for administrator rights when the installer has none.
; /UPDATE /OPEN=1 (HeMiMizer updates itself): quiet, and HeMiMizer starts again afterwards. A quiet installation without
; /OPEN=1 (HeMi Launcher) does not start it.
; The portable HeMiMizer.exe stays as it is: it runs from any folder (a USB stick) and updates itself in place.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef AppExe
  #define AppExe "..\build\release\HeMiMizer.exe"
#endif
#ifndef OutputDir
  #define OutputDir "..\build\dist"
#endif

[Setup]
AppId=HeMiMizer
AppName=HeMiMizer
AppVersion={#AppVersion}
AppVerName=HeMiMizer {#AppVersion}
AppPublisher=Richard Hemala
AppPublisherURL=https://github.com/Hemal1k/HeMiMizer
AppSupportURL=https://github.com/Hemal1k/HeMiMizer/issues
AppUpdatesURL=https://github.com/Hemal1k/HeMiMizer/releases
DefaultDirName={autopf}\HeMi\HeMiMizer
UsePreviousAppDir=no
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
; the first page asks: for me only (default, no administrator) or for all users (Program Files, asks for administrator)
PrivilegesRequiredOverridesAllowed=dialog commandline
; x64 only (the author's choice, 2026-11-18); Windows 11 on ARM runs it emulated
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename=HeMiMizer-Setup
SetupIconFile=..\assets\HeMiMizer.ico
UninstallDisplayIcon={app}\HeMiMizer.exe
UninstallDisplayName=HeMiMizer
Compression=lzma2/max
SolidCompression=yes
; the look of Windows 11, light or dark as Windows, as the installers of the other HeMi apps
WizardStyle=modern dynamic windows11
WizardSizePercent=120
CloseApplications=force
LicenseFile=..\LICENSE
; the language of Windows without asking (English when Windows is neither in English nor in Czech)
ShowLanguageDialog=auto

[Languages]
; English, and Czech when Windows is in Czech (the installer only - HeMiMizer itself is in English)
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "cs"; MessagesFile: "compiler:Languages\Czech.isl"

[CustomMessages]
en.DeleteSettings=Delete the settings of HeMiMizer too (its look, updates and log)?%n%nKeep them to use them again when you install HeMiMizer later.
cs.DeleteSettings=Smazat i nastavení HeMiMizeru (vzhled, aktualizace a log)?%n%nKdyž ho necháš, použije se znovu při další instalaci HeMiMizeru.

[Files]
Source: "{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion

[Tasks]
; offered on its own page, off by default
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Icons]
Name: "{autoprograms}\HeMiMizer"; Filename: "{app}\HeMiMizer.exe"
Name: "{autodesktop}\HeMiMizer"; Filename: "{app}\HeMiMizer.exe"; Tasks: desktopicon

[Run]
; HeMiMizer asks for administrator rights itself: started through the shell (CreateProcess cannot start it)
Filename: "{app}\HeMiMizer.exe"; Description: "{cm:LaunchProgram,HeMiMizer}"; Flags: nowait postinstall skipifsilent shellexec runasoriginaluser
; after its own update (/OPEN=1): the installer has the rights of the HeMiMizer that started it - no question again
Filename: "{app}\HeMiMizer.exe"; Parameters: "--updated"; Flags: nowait shellexec; Check: WizardSilent and OpenAfter

[Registry]
; what Windows itself keeps about the app (its name in menus)
Root: HKCU; Subkey: "Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache"; ValueType: none; ValueName: "{app}\HeMiMizer.exe.FriendlyAppName"; Flags: uninsdeletevalue dontcreatekey
Root: HKCU; Subkey: "Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache"; ValueType: none; ValueName: "{app}\HeMiMizer.exe.ApplicationCompany"; Flags: uninsdeletevalue dontcreatekey

[Code]
// The installation goes to {autopf}\HeMi\HeMiMizer, as the other HeMi apps since 2026-11. A HeMiMizer running from the
// installed folder (or from another folder the uninstall entry names) ends first; an installation in another folder
// is deleted after the new one is in place.
var
  OldFolder: String;

// /OPEN=1: HeMiMizer updated itself - it starts again after the quiet installation
function OpenAfter: Boolean;
begin
  Result := ExpandConstant('{param:OPEN|0}') = '1';
end;

function FindOldFolder: String;
begin
  if RegQueryStringValue(HKA, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\HeMiMizer_is1', 'InstallLocation', Result) then
  begin
    Result := RemoveBackslashUnlessRoot(Result);
    if (CompareText(Result, ExpandConstant('{app}')) = 0) or (CompareText(ExtractFileName(Result), 'HeMiMizer') <> 0)
      or not FileExists(Result + '\HeMiMizer.exe') then
      Result := '';
  end
  else
    Result := '';
end;

// Ends every program running from the folder. The folder as a PowerShell string in single quotes: an apostrophe in it
// (C:\Users\O'Brien) doubled; compared as text by StartsWith, not by -like, where [ ] and * would be wildcards.
procedure StopIn(Folder: String);
var
  Code: Integer;
  Quoted: String;
begin
  if (Folder = '') or not DirExists(Folder) then Exit;
  Quoted := AddBackslash(Folder);
  StringChangeEx(Quoted, '''', '''''', True);
  Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    '-NoProfile -NonInteractive -Command "$d = ''' + Quoted + '''; Get-Process | Where-Object { $_.Path -and $_.Path.StartsWith($d, [StringComparison]::OrdinalIgnoreCase) } | Stop-Process -Force"',
    '', SW_HIDE, ewWaitUntilTerminated, Code);
end;

procedure StopOld;
begin
  OldFolder := FindOldFolder;
  StopIn(OldFolder);
  // the HeMiMizer that started its own update may still be ending
  StopIn(ExpandConstant('{app}'));
end;

procedure DeleteOld;
begin
  if OldFolder <> '' then DelTree(OldFolder, True, True, True);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  StopOld;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then DeleteOld;
end;

// Uninstalling: the settings of the app (%AppData%\HeMi\HeMiMizer) are deleted or kept, as the user chooses (a quiet
// uninstall keeps them). %AppData%\HeMi goes too when no other HeMi app has anything there.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Dir: String;
begin
  if CurUninstallStep <> usPostUninstall then Exit;
  Dir := ExpandConstant('{userappdata}\HeMi\HeMiMizer');
  if not DirExists(Dir) then Exit;
  if UninstallSilent then Exit;
  if MsgBox(CustomMessage('DeleteSettings'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
  begin
    DelTree(Dir, True, True, True);
    RemoveDir(ExpandConstant('{userappdata}\HeMi'));
  end;
end;
