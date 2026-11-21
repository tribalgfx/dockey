; Inno Setup-script voor Dockey. Bouwen met scripts\build-installer.ps1 (dat bouwt eerst dockey.exe en geeft
; AppVersion en SourceExe mee).
;
; Per gebruiker, geen beheerdersrechten: Dockey komt in %LOCALAPPDATA%\Programs\Dockey. Je instellingen (in
; %APPDATA%\Dockey) blijven bij een update staan.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\src-tauri\target\release\dockey.exe"
#endif

[Setup]
AppId={{27D2C6B1-FD1C-4A06-B7C9-20424D27D76D}
AppName=Dockey
AppVersion={#AppVersion}
AppPublisher=Dockey
AppCopyright=Copyright (C) 2026 tribalgfx. GPL-3.0-or-later.
DefaultDirName={localappdata}\Programs\Dockey
DefaultGroupName=Dockey
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\artifacts
OutputBaseFilename=Dockey-Setup-{#AppVersion}
SetupIconFile=..\src-tauri\icons\icon.ico
UninstallDisplayIcon={app}\dockey.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
; Dockey heeft geen gewoon venster dat op "sluiten" reageert; we sluiten het zelf netjes af (zie [Code]).
CloseApplications=no
; Niet twee installers tegelijk.
SetupMutex=Dockey.Setup
; Nederlands of Engels, net als de taal van Windows; alleen bij twijfel vraagt de installer het.
ShowLanguageDialog=auto

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "nl"; MessagesFile: "compiler:Languages\Dutch.isl"

[CustomMessages]
en.StartNow=Start Dockey now
nl.StartNow=Dockey nu starten
en.ForceQuit=Dockey doesn't respond to the request to close (older versions don't know it yet).%n%nStop Dockey now? The Windows taskbar is restored afterwards.
nl.ForceQuit=Dockey reageert niet op het verzoek om af te sluiten (oudere versies kennen dat nog niet).%n%nDockey nu stoppen? De Windows-taakbalk wordt daarna teruggezet.
en.StillRunning=Dockey is still running. Close it via right-click on the bar (Exit Dockey) and try again.
nl.StillRunning=Dockey draait nog. Sluit het af via rechtsklik op de balk (Dockey afsluiten) en probeer het opnieuw.
en.StillRunningUninstall=Dockey is still running, so it can't be removed. Close it via right-click on the bar (Exit Dockey) and try again.
nl.StillRunningUninstall=Dockey draait nog en kan daarom niet worden verwijderd. Sluit het af via rechtsklik op de balk (Dockey afsluiten) en probeer het opnieuw.
en.NoWebView2=Dockey needs the Microsoft Edge WebView2 Runtime, which doesn't seem to be installed. Windows 11 always has it; on Windows 10 you can download it from Microsoft (search for "WebView2 Runtime").%n%nInstall Dockey anyway?
nl.NoWebView2=Dockey heeft de Microsoft Edge WebView2-runtime nodig, en die lijkt niet geïnstalleerd. Windows 11 heeft hem altijd; op Windows 10 download je hem bij Microsoft (zoek op "WebView2 Runtime").%n%nDockey toch installeren?
en.DeleteData=Do you also want to delete your Dockey settings?%nThat is the data in %1, including linked calendar links.
nl.DeleteData=Wil je ook je Dockey-instellingen verwijderen?%nDat zijn de gegevens in %1, ook de gekoppelde agenda-links.

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "dockey.exe"; Flags: ignoreversion
; De GPL vraagt de licentietekst mee te geven met het programma.
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Dockey"; Filename: "{app}\dockey.exe"

[Run]
Filename: "{app}\dockey.exe"; Description: "{cm:StartNow}"; Flags: nowait postinstall skipifsilent

[Code]
const
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';
  ApprovedKey = 'Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run';
  { Mutexen van een draaiende Dockey: de eigen (RUNNING_MUTEX in main.rs) en, voor versies van vóór die eigen
    mutex, die van de single-instance-plug-in van Tauri (identifier uit tauri.conf.json plus "-sim"). }
  RunningMutexes = 'Dockey.Running,nl.donkie.dockey-sim';
  WebView2Key = 'Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}';

function IsRunning: Boolean;
begin
  Result := CheckForMutexes(RunningMutexes);
end;

function TaskbarStateFile: String;
begin
  Result := ExpandConstant('{userappdata}\Dockey\taakbalk-origineel.txt');
end;

function WaitUntilStopped(Tries: Integer): Boolean;
var
  I: Integer;
begin
  for I := 1 to Tries do
  begin
    if not IsRunning then break;
    Sleep(250);
  end;
  Result := not IsRunning;
end;

{ Vraagt een draaiende Dockey zich netjes af te sluiten, zodat hij de Windows-taakbalk terugzet. Exe is een
  dockey.exe die --quit kent: het verzoek gaat naar de Dockey die draait, waar die ook staat. }
function QuitRunningDockey(Exe: String): Boolean;
var
  ResultCode: Integer;
begin
  if FileExists(Exe) then
    Exec(Exe, '--quit', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Result := WaitUntilStopped(40);
end;

{ Harde stop, voor een Dockey die --quit niet kent (oudere versies). Daarna staat de taakbalk nog verborgen:
  Exe --restore zet de bewaarde stand terug (alleen als die er is). }
function ForceQuitDockey(Exe: String): Boolean;
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM dockey.exe', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  Result := WaitUntilStopped(20);
  if Result and FileExists(TaskbarStateFile) and FileExists(Exe) then
    Exec(Exe, '--restore', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

{ Stopt een draaiende Dockey: eerst netjes, en als dat niet lukt na een vraag hard (bij een stille installatie
  meteen hard). Geeft False als Dockey blijft draaien. }
function StopDockey(Exe: String): Boolean;
begin
  Result := True;
  if not IsRunning then exit;
  if QuitRunningDockey(Exe) then exit;
  Result := (SuppressibleMsgBox(CustomMessage('ForceQuit'), mbConfirmation, MB_YESNO, IDYES) = IDYES) and ForceQuitDockey(Exe);
end;

function HasWebView2: Boolean;
var
  Version: String;
begin
  Result :=
    (RegQueryStringValue(HKLM, 'SOFTWARE\WOW6432Node\' + WebView2Key, 'pv', Version) and (Version <> '') and (Version <> '0.0.0.0')) or
    (RegQueryStringValue(HKLM, 'SOFTWARE\' + WebView2Key, 'pv', Version) and (Version <> '') and (Version <> '0.0.0.0')) or
    (RegQueryStringValue(HKCU, 'Software\' + WebView2Key, 'pv', Version) and (Version <> '') and (Version <> '0.0.0.0'));
end;

function InitializeSetup: Boolean;
begin
  Result := True;
  if not HasWebView2 then
    Result := SuppressibleMsgBox(CustomMessage('NoWebView2'), mbConfirmation, MB_YESNO, IDYES) = IDYES;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  if not IsRunning then exit;
  { De nieuwe dockey.exe kent --quit en vindt de draaiende Dockey, ook als die ergens anders staat. }
  ExtractTemporaryFile('dockey.exe');
  if not StopDockey(ExpandConstant('{tmp}\dockey.exe')) then
    Result := CustomMessage('StillRunning');
end;

function InitializeUninstall: Boolean;
begin
  Result := StopDockey(ExpandConstant('{app}\dockey.exe'));
  { Niet half verwijderen terwijl Dockey draait: dockey.exe zou blijven staan. }
  if not Result then
    SuppressibleMsgBox(CustomMessage('StillRunningUninstall'), mbError, MB_OK, IDOK);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  ResultCode: Integer;
begin
  { Is Dockey ooit hard gestopt, dan staat de oorspronkelijke taakbalkstand nog in dit bestand: zet die terug
    zolang dockey.exe er nog is. Zonder dat bestand is er niets terug te zetten. }
  if (CurUninstallStep = usUninstall) and not IsRunning and FileExists(TaskbarStateFile) then
    Exec(ExpandConstant('{app}\dockey.exe'), '--restore', '', SW_HIDE, ewWaitUntilTerminated, ResultCode);

  if CurUninstallStep <> usPostUninstall then exit;

  { "Starten met Windows" weghalen. }
  RegDeleteValue(HKEY_CURRENT_USER, RunKey, 'Dockey');
  RegDeleteValue(HKEY_CURRENT_USER, ApprovedKey, 'Dockey');

  { Instellingen blijven standaard staan (ook bij een stille verwijdering). De WebView2-gegevens horen erbij. }
  if SuppressibleMsgBox(FmtMessage(CustomMessage('DeleteData'), [ExpandConstant('{userappdata}\Dockey')]),
      mbConfirmation, MB_YESNO, IDNO) = IDYES then
  begin
    DelTree(ExpandConstant('{userappdata}\Dockey'), True, True, True);
    DelTree(ExpandConstant('{localappdata}\nl.donkie.dockey'), True, True, True);
  end;
end;
