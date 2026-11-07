#ifndef Payload
  #error Payload is required
#endif
#ifndef ReleaseVersion
  #error ReleaseVersion is required
#endif

[Setup]
AppId=Nerv.Compiler
AppName=Nerv
AppVersion={#ReleaseVersion}
AppPublisher=Nerv contributors
DefaultDirName={localappdata}\Programs\Nerv
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#OutputDirectory}
OutputBaseFilename=nerv-{#ReleaseVersion}-windows-x64-setup-unsigned
VersionInfoVersion={#NumericVersion}
VersionInfoProductTextVersion={#ReleaseVersion}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
DisableProgramGroupPage=yes
ChangesEnvironment=yes
CloseApplications=yes
RestartApplications=no
LicenseFile={#Payload}\LICENSE
UninstallDisplayIcon={app}\nerv.exe

[Tasks]
Name: addpath; Description: "Add Nerv to your user PATH"; Flags: checkedonce

[Files]
Source: "{#Payload}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Code]
function SameDirectory(A, B: String): Boolean;
begin
  Result := CompareText(RemoveBackslashUnlessRoot(A), RemoveBackslashUnlessRoot(B)) = 0;
end;

procedure RemoveOwnedPath(Directory: String);
var
  Owned, Value, Updated: String;
  Entries: TArrayOfString;
  I: Integer;
  HasEntry: Boolean;
begin
  if not RegQueryStringValue(HKCU, 'Software\Nerv', 'AddedPath', Owned) then exit;
  if not SameDirectory(Owned, Directory) then exit;
  if RegQueryStringValue(HKCU, 'Environment', 'Path', Value) then begin
    Entries := StringSplit(Value, [';'], stAll);
    Updated := '';
    HasEntry := False;
    for I := 0 to GetArrayLength(Entries) - 1 do begin
      if not SameDirectory(Entries[I], Owned) then begin
        if HasEntry then Updated := Updated + ';';
        Updated := Updated + Entries[I];
        HasEntry := True;
      end;
    end;
    if Updated <> Value then
      if not RegWriteExpandStringValue(HKCU, 'Environment', 'Path', Updated) then
        RaiseException('Could not update your user PATH.');
  end;
  RegDeleteValue(HKCU, 'Software\Nerv', 'AddedPath');
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Value, Directory, Owned: String;
  Entries: TArrayOfString;
  I: Integer;
begin
  if CurStep <> ssPostInstall then exit;
  Directory := ExpandConstant('{app}');
  if RegQueryStringValue(HKCU, 'Software\Nerv', 'AddedPath', Owned) then
    if not SameDirectory(Owned, Directory) then RemoveOwnedPath(Owned);
  if not WizardIsTaskSelected('addpath') then exit;
  RegQueryStringValue(HKCU, 'Environment', 'Path', Value);
  Entries := StringSplit(Value, [';'], stAll);
  for I := 0 to GetArrayLength(Entries) - 1 do
    if SameDirectory(Entries[I], Directory) then exit;
  if Value <> '' then Value := Value + ';';
  if not RegWriteExpandStringValue(HKCU, 'Environment', 'Path', Value + Directory) then
    RaiseException('Could not add Nerv to your user PATH.');
  if not RegWriteStringValue(HKCU, 'Software\Nerv', 'AddedPath', Directory) then
    RaiseException('Could not record Nerv PATH ownership.');
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then RemoveOwnedPath(ExpandConstant('{app}'));
end;
