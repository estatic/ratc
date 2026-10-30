# Build the on-screen overlay plugin and install it into X-Plane 11 as
# Resources\plugins\atc-overlay\64\win.xpl, with the follow-me car's roof object (sign,
# beacon, turn signals) in Resources\plugins\atc-overlay\objects\. Windows twin of
# install-xplane-plugin.sh. No administrator rights needed unless X-Plane sits in a protected
# folder (Program Files); then run from an elevated PowerShell.
# Usage: powershell -ExecutionPolicy Bypass -File tools\install-xplane-plugin.ps1 [X-Plane dir]
#        [-NoBuild] (install target\release\atc_overlay.dll as built, e.g. from a release zip)
param(
    [string]$XPlane = (Join-Path ${env:ProgramFiles(x86)} 'Steam\steamapps\common\X-Plane 11'),
    [switch]$NoBuild
)
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $XPlane 'Resources\plugins'))) {
    throw "no X-Plane 11 at $XPlane (pass its directory as the argument)"
}
# A release archive ships the plugin already built, next to tools\.
$prebuilt = Join-Path $Root 'plugin\atc-overlay\64\win.xpl'
if (Test-Path $prebuilt) {
    $dll = $prebuilt
} else {
    if (-not $NoBuild) {
        $ErrorActionPreference = 'Continue'
        cargo build --release -p atc-xplane-plugin --manifest-path (Join-Path $Root 'Cargo.toml')
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }
        $ErrorActionPreference = 'Stop'
    }
    $dll = Join-Path $Root 'target\release\atc_overlay.dll'
}
$dest = Join-Path $XPlane 'Resources\plugins\atc-overlay\64'
$objects = Join-Path $XPlane 'Resources\plugins\atc-overlay\objects'
$assets = Join-Path $Root 'crates\atc-xplane-plugin\assets'
if (-not (Test-Path $assets)) { $assets = Join-Path $Root 'plugin\atc-overlay\objects' }
try {
    New-Item -ItemType Directory -Force $dest, $objects | Out-Null
    Copy-Item -Force $dll (Join-Path $dest 'win.xpl')
    Copy-Item -Force (Join-Path $assets 'follow_me_roof.obj'), (Join-Path $assets 'follow_me_roof.png') $objects
} catch [UnauthorizedAccessException] {
    throw "no permission to write into $XPlane; run this script from an elevated PowerShell"
}
Write-Host "installed: $dest\win.xpl and $objects\follow_me_roof.{obj,png} (restart X-Plane to load them)"
