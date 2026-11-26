; Installer Windows. Si compila con: ISCC /DAppVersion=x.y.z packaging\renamebot.iss
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6BDAC044-8B54-4DD1-BC3C-D360C4440C03}
AppName=RenameBot
AppVersion={#AppVersion}
AppPublisher=Emiliano Chirchiano
DefaultDirName={autopf}\RenameBot
DefaultGroupName=RenameBot
OutputDir=..\dist
OutputBaseFilename=RenameBot-{#AppVersion}-setup
SetupIconFile=..\assets\renamebot.ico
UninstallDisplayIcon={app}\renamebot.ico
LicenseFile=..\LICENSE
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequiredOverridesAllowed=dialog
WizardStyle=modern

[Languages]
Name: "it"; MessagesFile: "compiler:Languages\Italian.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\release\renamebot.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\renamebot.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\RenameBot"; Filename: "{app}\renamebot.exe"; IconFilename: "{app}\renamebot.ico"
Name: "{autodesktop}\RenameBot"; Filename: "{app}\renamebot.exe"; IconFilename: "{app}\renamebot.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\renamebot.exe"; Description: "{cm:LaunchProgram,RenameBot}"; Flags: nowait postinstall skipifsilent
