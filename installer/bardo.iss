; Bardo's Windows installer (issue #135), built with Inno Setup 6 by
; `cargo xtask package`, which passes AppVersion, StageDir (the installed
; layout it staged), OutputDir and RepoRoot.
;
; Per user, no administrator: %LOCALAPPDATA%\Programs\Bardo. On a first
; install it asks the background agent question (ADR-0006) and leaves the
; answer, and an optional settings file to import (ADR-0014), in
; %APPDATA%\Bardo\setup-choices.toml, which Bardo reads at its next start;
; Bardo registers the agent's task itself. The uninstaller runs
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
; The settings file page (ADR-0014), on a first install only.
en.SettingsCaption=Import settings
ptbr.SettingsCaption=Importar configurações
en.SettingsDescription=Bring settings from another copy of Bardo.
ptbr.SettingsDescription=Traga configurações de outra cópia do Bardo.
en.SettingsHelp=If you exported a settings file from another copy of Bardo (Settings › Backup › Settings file), pick it here. The next time Bardo opens it asks for the file's password and lets you tick what to import. Leave it empty to start fresh.
ptbr.SettingsHelp=Se você exportou um arquivo de configurações de outra cópia do Bardo (Configurações › Backup › Arquivo de configurações), escolha-o aqui. Na próxima vez que abrir, o Bardo pede a senha do arquivo e deixa você marcar o que importar. Deixe em branco para começar do zero.
en.SettingsFile=Settings file (optional):
ptbr.SettingsFile=Arquivo de configurações (opcional):
en.SettingsFilter=Bardo settings (*.bardo-settings)|*.bardo-settings|All files|*.*
ptbr.SettingsFilter=Configurações do Bardo (*.bardo-settings)|*.bardo-settings|Todos os arquivos|*.*
en.SettingsMissing=That file does not exist. Pick another or leave the field empty.
ptbr.SettingsMissing=Esse arquivo não existe. Escolha outro ou deixe o campo em branco.
en.CloseBardo=Bardo is open. Close it, then select Retry to uninstall.
ptbr.CloseBardo=O Bardo está aberto. Feche o Bardo e selecione Repetir para desinstalar.

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "backgroundagent"; Description: "{cm:AgentTask}"; GroupDescription: "{cm:AgentGroup}"; Flags: unchecked; Check: IsFirstInstall

[InstallDelete]
; A newer ffmpeg may drop a DLL: never leave an old one beside the new.
Type: filesandordirs; Name: "{app}\ffmpeg"
; Same for the vision models: a newer pin may rename or drop one.
Type: filesandordirs; Name: "{app}\models"

[Files]
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\Bardo"; Filename: "{app}\bardo.exe"
Name: "{autodesktop}\Bardo"; Filename: "{app}\bardo.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\bardo.exe"; Description: "{cm:LaunchProgram,Bardo}"; Flags: nowait postinstall skipifsilent
; An update Bardo started (issue #144) runs this silently with /RELAUNCH=1
; after Bardo quit: Bardo opens again once its files are in place.
Filename: "{app}\bardo.exe"; Flags: nowait; Check: RelaunchAfterUpdate

[UninstallRun]
; Turns the background agent off, removes its task and waits for it to
; stop, so its files can go. Projects, the database and keys stay.
Filename: "{app}\bardo.exe"; Parameters: "--uninstall"; Flags: runhidden waituntilterminated; RunOnceId: "TurnOffAgent"

[Code]
const
  UninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{{#AppGuid}}_is1';

var
  FirstInstall: Boolean;
  SettingsPage: TInputFileWizardPage;

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

// An optional settings file to import (ADR-0014): Bardo asks its password
// at its next start, so the installer never sees it.
procedure InitializeWizard();
begin
  SettingsPage := CreateInputFilePage(wpSelectTasks,
    CustomMessage('SettingsCaption'), CustomMessage('SettingsDescription'),
    CustomMessage('SettingsHelp'));
  SettingsPage.Add(CustomMessage('SettingsFile'),
    CustomMessage('SettingsFilter'), '.bardo-settings');
end;

function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := (PageID = SettingsPage.ID) and not FirstInstall;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
var
  Chosen: String;
begin
  Result := True;
  if CurPageID = SettingsPage.ID then
  begin
    Chosen := Trim(SettingsPage.Values[0]);
    if (Chosen <> '') and not FileExists(Chosen) then
    begin
      MsgBox(CustomMessage('SettingsMissing'), mbError, MB_OK);
      Result := False;
    end;
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Dir, Answer, Chosen: String;
  Lines: TArrayOfString;
begin
  if (CurStep = ssPostInstall) and FirstInstall then
  begin
    if WizardIsTaskSelected('backgroundagent') then
      Answer := 'true'
    else
      Answer := 'false';
    SetArrayLength(Lines, 2);
    Lines[0] := '# Left by the Bardo installer. Bardo reads it once at its next start.';
    Lines[1] := 'background_agent = ' + Answer;
    Chosen := Trim(SettingsPage.Values[0]);
    if Chosen <> '' then
    begin
      // A TOML multi-line literal string: no escapes, quotes allowed.
      SetArrayLength(Lines, 3);
      Lines[2] := 'settings_file = ' + #39#39#39 + Chosen + #39#39#39;
    end;
    Dir := ExpandConstant('{userappdata}\Bardo');
    ForceDirectories(Dir);
    // UTF-8, so a path with accents reaches Bardo intact.
    SaveStringsToUTF8File(Dir + '\setup-choices.toml', Lines, False);
  end;
end;

// Bardo runs its update's installer with /SILENT ... /RELAUNCH=1 and quits
// (ADR-0011); a silent install from the command line leaves Bardo closed.
function RelaunchAfterUpdate(): Boolean;
var
  I: Integer;
begin
  Result := False;
  if not WizardSilent then
    Exit;
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), '/RELAUNCH=1') = 0 then
    begin
      Result := True;
      Exit;
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
