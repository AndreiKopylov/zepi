; Zepi setup: per-machine install into Program Files with selectable components.
#define AppName "Zepi"
#define AppExeName "zepi.exe"
#define AppPublisher "Zepi"
#ifndef AppVersion
  #define AppVersion "0.0.1-beta1"
#endif
#define AppId "{{6F1D6E5A-8F14-4E2C-9B7A-2E4C9F0B31D7}"
#define StagingDir "..\dist\staging"

[Setup]
AppId={#AppId}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL=https://github.com/AndreiKopylov/zepi
DefaultDirName={autopf}\{#AppName}
DisableDirPage=no
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\dist
OutputBaseFilename=zepi-{#AppVersion}-setup
SetupIconFile=..\native\overlay\crates\zed\resources\windows\app-icon-dev.ico
UninstallDisplayIcon={app}\{#AppExeName}
UninstallDisplayName={#AppName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
AppMutex=Zepi-Editor-Dev-Instance-Mutex
CloseApplications=force
RestartApplications=no
AllowNoIcons=yes
UsePreviousTasks=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "russian"; MessagesFile: "compiler:Languages\\Russian.isl"

; The default Inno texts repeat the page title three times; keep one short hint.
[Messages]
english.SelectComponentsDesc=Pick what you need. Every component can be added later by running this setup again.
russian.SelectComponentsDesc=Отметьте нужное. Любой компонент можно добавить позже, запустив установщик снова.
english.SelectComponentsLabel2=
russian.SelectComponentsLabel2=
english.SelectTasksLabel2=
russian.SelectTasksLabel2=

[CustomMessages]
english.CompCore={#AppName} editor and agent
russian.CompCore=Редактор и агент {#AppName}
english.CompOffice=Python for Office documents: Word, Excel, PowerPoint
russian.CompOffice=Python для документов Office: Word, Excel, PowerPoint
english.CompOfficeDesc=Bundled Python 3.13 with python-docx, openpyxl and python-pptx — 56 MB
russian.CompOfficeDesc=Встроенный Python 3.13 с python-docx, openpyxl и python-pptx — 56 МБ
english.CompBrowser=Built-in browser for the agent
russian.CompBrowser=Встроенный браузер для агента
english.CompBrowserDesc=Bundled Chromium for pages that need a real browser — 702 MB
russian.CompBrowserDesc=Встроенный Chromium для страниц, которым нужен настоящий браузер — 702 МБ
english.RemoveUserData=Also delete your Zepi settings, sessions and bundled skills from %LOCALAPPDATA%\Zepi?%n%nChoose No to keep them for a future reinstall.
russian.RemoveUserData=Удалить также настройки, сессии и скиллы из %LOCALAPPDATA%\Zepi?%n%nВыберите «Нет», чтобы сохранить их для будущей установки.

[Components]
Name: "core"; Description: "{cm:CompCore}"; Flags: fixed
Name: "office"; Description: "{cm:CompOffice}"; Flags: checkablealone
Name: "browser"; Description: "{cm:CompBrowser}" ; Flags: checkablealone

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#StagingDir}\zepi.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#StagingDir}\packages\*"; DestDir: "{app}\packages"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#StagingDir}\node_modules\*"; DestDir: "{app}\node_modules"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#StagingDir}\native\*"; DestDir: "{app}\native"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#StagingDir}\deps\node\*"; DestDir: "{app}\deps\node"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#StagingDir}\deps\python\*"; DestDir: "{app}\deps\python"; Flags: ignoreversion recursesubdirs createallsubdirs; Components: office
Source: "{#StagingDir}\deps\browsers\*"; DestDir: "{app}\deps\browsers"; Flags: ignoreversion recursesubdirs createallsubdirs; Components: browser

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExeName}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: files; Name: "{app}\zepi-root.txt"

[Code]
procedure InitializeWizard;
var
  I: Integer;
begin
  { No "custom installation" dropdown: the component list below is the only choice. }
  WizardForm.TypesCombo.Visible := False;
  for I := 0 to WizardForm.ComponentsList.Items.Count - 1 do
    WizardForm.ComponentsList.Checked[I] := True;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataDir: String;
begin
  if CurUninstallStep <> usPostUninstall then
    Exit;
  DataDir := ExpandConstant('{localappdata}\Zepi');
  if not DirExists(DataDir) then
    Exit;
  if MsgBox(CustomMessage('RemoveUserData'), mbConfirmation, MB_YESNO) = IDYES then
    DelTree(DataDir, True, True, True);
end;
