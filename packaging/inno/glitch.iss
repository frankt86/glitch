#define AppName "Glitch"
#define AppVersion "0.1.0"
#define AppPublisher "Terry James"
#define AppURL "https://github.com/frankt86/glitch"
#define AppExeName "glitch.exe"

[Setup]
AppId={{A1B2C3D4-E5F6-7890-ABCD-EF1234567890}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppURL}
AppSupportURL={#AppURL}
AppUpdatesURL={#AppURL}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
AllowNoIcons=yes
; Output
OutputDir=..\..\build
OutputBaseFilename=GlitchSetup
; Installer icon
SetupIconFile=..\..\app\glitch\assets\app_icon.ico
; Compression
Compression=lzma2/ultra64
SolidCompression=yes
; Appearance
WizardStyle=modern
; Require admin for Program Files install
PrivilegesRequired=admin
; Architecture
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Minimum Windows version: Windows 11 (WebView2 pre-installed)
MinVersion=10.0.22000

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\..\target\release\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}";          Filename: "{app}\{#AppExeName}"
Name: "{group}\Uninstall {#AppName}"; Filename: "{uninstallexe}"
Name: "{commondesktop}\{#AppName}";  Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Run]
; Install Git if not already present (required for vault sync).
Filename: "{sys}\cmd.exe"; Parameters: "/c winget install --id Git.Git --silent --accept-source-agreements --accept-package-agreements"; Check: NeedsGit; StatusMsg: "Installing Git..."; Flags: runhidden waituntilterminated
; Install Claude Code CLI if not already present.
Filename: "{sys}\cmd.exe"; Parameters: "/c winget install --id Anthropic.ClaudeCode --silent --accept-source-agreements --accept-package-agreements"; Check: NeedsClaude; StatusMsg: "Installing Claude Code CLI..."; Flags: runhidden waituntilterminated
; Launch Glitch after install.
Filename: "{app}\{#AppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(AppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent

[Code]
// Returns true if ExeName resolves via PATH (i.e. already installed).
function IsOnPath(const ExeName: String): Boolean;
var
  ResultCode: Integer;
begin
  Result := Exec(ExpandConstant('{sys}\cmd.exe'),
                 '/c where ' + ExeName + ' >nul 2>&1',
                 '', SW_HIDE, ewWaitUntilTerminated, ResultCode)
            and (ResultCode = 0);
end;

function NeedsGit: Boolean;
begin
  Result := not IsOnPath('git');
end;

function NeedsClaude: Boolean;
begin
  Result := not IsOnPath('claude');
end;
