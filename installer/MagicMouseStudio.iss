#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

#define AppName "Magic Mouse Studio"
#define AppPublisher "Magic Mouse Studio"
#define AppURL "https://github.com/Victor-Gouin/Magic-Mouse-Utilities"
#define AppExeName "magic-mouse-win.exe"

[Setup]
AppId={{8F5E3A2B-7B1D-4C2E-9F1A-6D8E4A2C3B10}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppURL}
AppSupportURL={#AppURL}/issues
AppUpdatesURL={#AppURL}/releases
; Installation par utilisateur pour garder magic-mouse-config.json modifiable sans droits admin
PrivilegesRequired=lowest
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
AllowNoIcons=yes
OutputDir=output
OutputBaseFilename=MagicMouseStudio-Setup-{#AppVersion}
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes
UninstallDisplayIcon={app}\{#AppExeName}

[Languages]
Name: "french"; MessagesFile: "compiler:Languages\French.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
french.AutoStartTask=Lancer automatiquement Magic Mouse Studio au démarrage de Windows (zone de notification)
english.AutoStartTask=Start Magic Mouse Studio automatically with Windows (system tray)

[Tasks]
Name: "autostart"; Description: "{cm:AutoStartTask}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\release\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\certs\MagicMouseStudio-CodeSigning.cer"; DestDir: "{app}\certs"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{group}\{cm:UninstallProgram,{#AppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "MagicMouseStudio"; ValueData: """{app}\{#AppExeName}"" --tray"; Flags: uninsdeletevalue; Tasks: autostart

[Run]
Filename: "{app}\{#AppExeName}"; Description: "{cm:LaunchProgram,{#StringChange(AppName, '&', '&&')}}"; Flags: nowait postinstall skipifsilent
