#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef SourceDir
  #error SourceDir is required
#endif
#ifndef OutputDir
  #error OutputDir is required
#endif

[Setup]
AppId={{7BF7F74E-F3E6-47F5-AD77-421EE0D18C75}
AppName=f0lock
AppVersion={#AppVersion}
AppPublisher=sout
AppPublisherURL=https://github.com/sout233/f0lock
DefaultDirName={autopf}\f0lock
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutputDir}
OutputBaseFilename=f0lock-{#AppVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
UninstallDisplayName=f0lock

[Files]
Source: "{#SourceDir}\f0lock.clap"; DestDir: "{commoncf64}\CLAP"; Flags: ignoreversion
Source: "{#SourceDir}\f0lock.vst3\*"; DestDir: "{commoncf64}\VST3\f0lock.vst3"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "{#SourceDir}\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\INSTALL.txt"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\licenses\*"; DestDir: "{app}\licenses"; Flags: ignoreversion
