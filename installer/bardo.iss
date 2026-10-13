; Bardo's Windows installer (issue #135), built with Inno Setup 6 by
; `cargo xtask package`, which passes AppVersion, StageDir (the installed
; layout it staged), OutputDir and RepoRoot.
;
; Per user, no administrator: %LOCALAPPDATA%\Programs\Bardo. On a first
; install it asks the background agent question (ADR-0006) and leaves the
; answer in %APPDATA%\Bardo\setup-choices.toml, which Bardo reads at its
; next start; Bardo registers the agent's task itself. The uninstaller runs
; `bardo --uninstall` (agent off, task removed) and keeps the user's data.

#ifndef AppVersion
  #error AppVersion is not defined: build with cargo xtask package
#endif
#ifndef StageDir
  #error StageDir is not defined: build with cargo xtask package
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif
#ifndef RepoRoot
  #define RepoRoot ".."
#endif

; Never change it: upgrades and the uninstall entry are found by it.
#define AppGuid "C6E790A6-186B-4ED1-8997-82360B3BE325"

[Setup]
AppId={{{#AppGuid}}
AppName=Bardo
AppVersion={#AppVersion}
AppVerName=Bardo {#AppVersion}
AppPublisher=Bardo
AppPublisherURL=https://github.com/fernandolisboa/bardo
AppSupportURL=https://fernandolisboa.github.io/bardo/
VersionInfoVersion={#AppVersion}
VersionInfoDescription=Bardo Setup
DefaultDirName={autopf}\Bardo
DefaultGroupName=Bardo
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
WizardStyle=modern
SetupIconFile={#RepoRoot}\assets\brand\bardo.ico
UninstallDisplayIcon={app}\bardo.exe
UninstallDisplayName=Bardo
OutputDir={#OutputDir}
OutputBaseFilename=Bardo-Setup-{#AppVersion}-x64
Compression=lzma2/max
SolidCompression=yes
; A running Bardo, or its background agent (which has no window), holds
; bardo.exe and the ffmpeg DLLs: close them before files are replaced. The
; agent hands its jobs back through their leases and starts again at the
; next sign-in or within 15 minutes.
CloseApplications=force
RestartApplications=no
LanguageDetectionMethod=uilanguage
ShowLanguageDialog=auto
; Code signing goes here when Bardo has a certificate:
; SignTool=...

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "ptbr"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[CustomMessages]
; The background agent question, as ADR-0006 words it.
en.AgentGroup=Publish even when Bardo is closed? Installs a lightweight agent that sends scheduled posts in the background while you are signed in to Windows. You can turn it off later in Settings.
ptbr.AgentGroup=Publicar mesmo com o Bardo fechado? Instala um agente leve que envia os posts agendados em segundo plano enquanto você estiver conectado ao Windows. Dá para desligar depois nas configurações.
en.AgentTask=Publish scheduled posts while Bardo is closed
ptbr.AgentTask=Publicar os posts agendados com o Bardo fechado
en.CloseBardo=Bardo is open. Close it, then select Retry to uninstall.
ptbr.CloseBardo=O Bardo está aberto. Feche o Bardo e selecione Repetir para desinstalar.

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "backgroundagent"; Description: "{cm:AgentTask}"; GroupDescription: "{cm:AgentGroup}"; Flags: unchecked; Check: IsFirstInstall

[InstallDelete]
; A newer ffmpeg may drop a DLL: never leave an old one beside the new.
Type: filesandordirs; Name: "{app}\ffmpeg"

[Files]
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\Bardo"; Filename: "{app}\bardo.exe"
Name: "{autodesktop}\Bardo"; Filename: "{app}\bardo.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\bardo.exe"; Description: "{cm:LaunchProgram,Bardo}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; Turns the background agent off, removes its task and waits for it to
; stop, so its files can go. Projects, the database and keys stay.
Filename: "{app}\bardo.exe"; Parameters: "--uninstall"; Flags: runhidden waituntilterminated; RunOnceId: "TurnOffAgent"

[Code]
const
  UninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{{#AppGuid}}_is1';

var
  FirstInstall: Boolean;

function InitializeSetup(): Boolean;
begin
  // Read before this run writes its own uninstall entry.
  FirstInstall := not RegKeyExists(HKCU, UninstallKey);
  Result := True;
end;

// The agent question is asked once; an upgrade keeps the user's setting.
function IsFirstInstall(): Boolean;
begin
  Result := FirstInstall;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Dir, Answer: String;
begin
  if (CurStep = ssPostInstall) and FirstInstall then
  begin
    if WizardIsTaskSelected('backgroundagent') then
      Answer := 'true'
    else
      Answer := 'false';
    Dir := ExpandConstant('{userappdata}\Bardo');
    ForceDirectories(Dir);
    SaveStringToFile(Dir + '\setup-choices.toml',
      '# Left by the Bardo installer. Bardo reads it once at its next start.'#13#10 +
      'background_agent = ' + Answer + #13#10, False);
  end;
end;

// Whether Bardo's window (not the background agent) runs from this folder.
function BardoWindowOpen(): Boolean;
var
  Locator, Service, Processes, Process: Variant;
  ProgramPath, CommandLine: String;
  I: Integer;
begin
  Result := False;
  ProgramPath := ExpandConstant('{app}\bardo.exe');
  try
    Locator := CreateOleObject('WbemScripting.SWbemLocator');
    Service := Locator.ConnectServer('.', 'root\CIMV2');
    Processes := Service.ExecQuery(
      'SELECT ExecutablePath, CommandLine FROM Win32_Process WHERE Name = ''bardo.exe''');
    for I := 0 to Processes.Count - 1 do
    begin
      Process := Processes.ItemIndex(I);
      if VarIsNull(Process.ExecutablePath) or VarIsNull(Process.CommandLine) then
        Continue;
      CommandLine := Process.CommandLine;
      if (CompareText(Process.ExecutablePath, ProgramPath) = 0) and
         (Pos('--agent', CommandLine) = 0) and (Pos('--uninstall', CommandLine) = 0) then
      begin
        Result := True;
        Exit;
      end;
    end;
  except
    // No WMI: the uninstaller goes on, and a file in use is removed at the
    // next restart.
    Result := False;
  end;
end;

function InitializeUninstall(): Boolean;
begin
  Result := True;
  while BardoWindowOpen() do
  begin
    if SuppressibleMsgBox(CustomMessage('CloseBardo'), mbError, MB_RETRYCANCEL, IDCANCEL) = IDCANCEL then
    begin
      Result := False;
      Exit;
    end;
  end;
end;
