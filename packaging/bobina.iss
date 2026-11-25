; Installer Windows. Si compila con: ISCC /DAppVersion=x.y.z packaging\bobina.iss
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{38D74B23-DD1F-4B2D-95F2-5F5F64E57AD2}
AppName=Bobina
AppVersion={#AppVersion}
AppPublisher=Emiliano Chirchiano
DefaultDirName={autopf}\Bobina
DefaultGroupName=Bobina
OutputDir=..\dist
OutputBaseFilename=Bobina-{#AppVersion}-setup
SetupIconFile=..\assets\bobina.ico
UninstallDisplayIcon={app}\bobina.ico
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
Source: "..\target\release\bobina.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\bobina.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Bobina"; Filename: "{app}\bobina.exe"; IconFilename: "{app}\bobina.ico"
Name: "{autodesktop}\Bobina"; Filename: "{app}\bobina.exe"; IconFilename: "{app}\bobina.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\bobina.exe"; Description: "{cm:LaunchProgram,Bobina}"; Flags: nowait postinstall skipifsilent
