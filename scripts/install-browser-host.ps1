param(
  [Parameter(Mandatory = $true)]
  [string]$ChromeExtensionId,

  [string]$EdgeExtensionId = $ChromeExtensionId,

  [string]$BraveExtensionId = $ChromeExtensionId,

  [string]$FirefoxExtensionId = "daytrail-browser-bridge@example.com",

  [string]$AppBin = "$env:LOCALAPPDATA\Programs\DayTrail\DayTrail.exe"
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($ChromeExtensionId) -or $ChromeExtensionId -eq "__EXTENSION_ID__") {
  throw "ChromeExtensionId must be the real installed extension id."
}

if ([string]::IsNullOrWhiteSpace($EdgeExtensionId) -or $EdgeExtensionId -eq "__EXTENSION_ID__") {
  throw "EdgeExtensionId must be the real installed extension id."
}

if ([string]::IsNullOrWhiteSpace($BraveExtensionId) -or $BraveExtensionId -eq "__EXTENSION_ID__") {
  throw "BraveExtensionId must be the real installed extension id."
}

if (-not (Test-Path -LiteralPath $AppBin)) {
  throw "DayTrail executable not found: $AppBin. Pass -AppBin with the installed executable path."
}

$rootDir = Resolve-Path (Join-Path $PSScriptRoot "..")
$hostName = "ai.daytrail.desktop"
$hostDir = Join-Path $env:LOCALAPPDATA "DayTrail\NativeMessaging"
$hostWrapper = Join-Path $hostDir "$hostName-native-host.cmd"
$chromeManifest = Join-Path $hostDir "$hostName.chrome.json"
$braveManifest = Join-Path $hostDir "$hostName.brave.json"
$edgeManifest = Join-Path $hostDir "$hostName.edge.json"
$firefoxManifest = Join-Path $hostDir "$hostName.firefox.json"

New-Item -ItemType Directory -Force -Path $hostDir | Out-Null

$wrapper = @"
@echo off
"$AppBin" --native-messaging-host %*
"@
Set-Content -LiteralPath $hostWrapper -Value $wrapper -Encoding ASCII

node (Join-Path $rootDir "scripts/write-native-host-manifest.mjs") chrome $hostWrapper $ChromeExtensionId $chromeManifest
node (Join-Path $rootDir "scripts/write-native-host-manifest.mjs") brave $hostWrapper $BraveExtensionId $braveManifest
node (Join-Path $rootDir "scripts/write-native-host-manifest.mjs") edge $hostWrapper $EdgeExtensionId $edgeManifest
node (Join-Path $rootDir "scripts/write-native-host-manifest.mjs") firefox $hostWrapper $FirefoxExtensionId $firefoxManifest

New-Item -Path "HKCU:\Software\Google\Chrome\NativeMessagingHosts\$hostName" -Force | Out-Null
Set-Item -Path "HKCU:\Software\Google\Chrome\NativeMessagingHosts\$hostName" -Value $chromeManifest

New-Item -Path "HKCU:\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\$hostName" -Force | Out-Null
Set-Item -Path "HKCU:\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\$hostName" -Value $braveManifest

New-Item -Path "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\$hostName" -Force | Out-Null
Set-Item -Path "HKCU:\Software\Microsoft\Edge\NativeMessagingHosts\$hostName" -Value $edgeManifest

New-Item -Path "HKCU:\Software\Mozilla\NativeMessagingHosts\$hostName" -Force | Out-Null
Set-Item -Path "HKCU:\Software\Mozilla\NativeMessagingHosts\$hostName" -Value $firefoxManifest

Write-Output $hostWrapper
Write-Output $chromeManifest
Write-Output $braveManifest
Write-Output $edgeManifest
Write-Output $firefoxManifest
