#!/usr/bin/env pwsh
# Windows build script for OpenixCLI
# Usage: ./scripts/build-windows.ps1 <target> <version>

param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [string]$Version = $env:RELEASE_TAG
)

$ErrorActionPreference = "Stop"

# Remove v prefix from version
$Version = $Version -replace '^v', ''

Write-Host "Building Windows package for target: $Target"
Write-Host "Version: $Version"

$BinaryPath = "target/$Target/release/openixcli.exe"
$ZipName = "openixcli-v$Version-$Target.zip"
$MsiName = "openixcli-v$Version-$Target.msi"
$NsisName = "openixcli-v$Version-$Target-setup.exe"

# Check if binary exists
if (-not (Test-Path $BinaryPath)) {
    Write-Host "Error: Binary not found at $BinaryPath"
    Write-Host "Please run 'cargo build --release --target $Target' first"
    exit 1
}

# Find libusb DLL
$LibusbDll = Get-ChildItem -Path "target/$Target/release/build" -Recurse -Filter "libusb-1.0.dll" -ErrorAction SilentlyContinue | Select-Object -First 1

if ($LibusbDll) {
    Write-Host "Found libusb DLL: $($LibusbDll.FullName)"
    # Copy to release directory for packaging
    Copy-Item $LibusbDll.FullName "target/$Target/release/" -Force
} else {
    Write-Host "Warning: libusb DLL not found"
}

# Create ZIP
Write-Host "Creating ZIP..."
$ReleaseDir = "release"
New-Item -ItemType Directory -Path $ReleaseDir -Force | Out-Null
Copy-Item $BinaryPath $ReleaseDir/
if ($LibusbDll) {
    Copy-Item "target/$Target/release/libusb-1.0.dll" $ReleaseDir/
}
Compress-Archive -Path "$ReleaseDir/*" -DestinationPath $ZipName -Force
Remove-Item -Recurse -Force $ReleaseDir
Write-Host "Created: $ZipName"

# Create MSI
Write-Host "Creating MSI..."
$WxsContent = @"
<?xml version="1.0" encoding="UTF-8"?>
<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs">
  <Package Name="OpenixCLI" Manufacturer="YuzukiTsuru" Version="$Version" UpgradeCode="E9F8A8C0-1D2E-4B3C-8F9A-5E6D7C8B9A0F" Language="1033">
    <MediaTemplate EmbedCab="yes" />
    <Feature Id="MainFeature" Title="OpenixCLI" Level="1">
      <ComponentRef Id="OpenixCLIComponent" />
      <ComponentRef Id="LibUSBComponent" />
    </Feature>
    <StandardDirectory Id="ProgramFiles6432Folder">
      <Directory Id="INSTALLFOLDER" Name="OpenixCLI">
        <Component Id="OpenixCLIComponent" Guid="*">
          <File Id="OpenixCLIExe" Source="target/$Target/release/openixcli.exe" KeyPath="yes" />
        </Component>
        <Component Id="LibUSBComponent" Guid="*">
          <File Id="LibUSBDll" Source="target/$Target/release/libusb-1.0.dll" />
        </Component>
      </Directory>
    </StandardDirectory>
  </Package>
</Wix>
"@
$WxsContent | Out-File -FilePath "openixcli.wxs" -Encoding UTF8
wix build openixcli.wxs -o $MsiName
Write-Host "Created: $MsiName"

# Create NSIS installer
Write-Host "Creating NSIS installer..."
$NsisContent = @"
!include "MUI2.nsh"
Name "OpenixCLI"
OutFile "$NsisName"
InstallDir `"`$PROGRAMFILES\OpenixCLI`"
InstallDirRegKey HKLM "Software\OpenixCLI" "Install_Dir"
RequestExecutionLevel admin

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_LICENSE "LICENSE"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_WELCOME
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_UNPAGE_FINISH

!insertmacro MUI_LANGUAGE "English"

Section "OpenixCLI" SecOpenixCLI
  SetOutPath "`$INSTDIR"
  File "target\$Target\release\openixcli.exe"
  File "target\$Target\release\libusb-1.0.dll"
  WriteRegStr HKLM "Software\OpenixCLI" "Install_Dir" "`$INSTDIR"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\OpenixCLI" "DisplayName" "OpenixCLI"
  WriteRegStr HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\OpenixCLI" "UninstallString" '"`$INSTDIR\uninstall.exe"'
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\OpenixCLI" "NoModify" 1
  WriteRegDWORD HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\OpenixCLI" "NoRepair" 1
  WriteUninstaller "`$INSTDIR\uninstall.exe"
SectionEnd

Section "Uninstall"
  Delete "`$INSTDIR\openixcli.exe"
  Delete "`$INSTDIR\libusb-1.0.dll"
  Delete "`$INSTDIR\uninstall.exe"
  RMDir "`$INSTDIR"
  DeleteRegKey HKLM "Software\Microsoft\Windows\CurrentVersion\Uninstall\OpenixCLI"
  DeleteRegKey HKLM "Software\OpenixCLI"
SectionEnd
"@
$NsisContent | Out-File -FilePath "installer.nsi" -Encoding UTF8
& "C:\Program Files (x86)\NSIS\makensis.exe" installer.nsi
Write-Host "Created: $NsisName"

Write-Host ""
Write-Host "Build complete!"
Write-Host "  ZIP: $ZipName"
Write-Host "  MSI: $MsiName"
Write-Host "  NSIS: $NsisName"