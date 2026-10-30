# Fetch the recommended X-Plane Scenery Gateway apt.dat for an airport (default EPKK).
# Windows twin of fetch-gateway-apt.sh.
# Usage: powershell -ExecutionPolicy Bypass -File tools\fetch-gateway-apt.ps1 [ICAO]
param([string]$Icao = 'EPKK')
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
Add-Type -AssemblyName System.IO.Compression
$Out = Join-Path (Split-Path -Parent $PSScriptRoot) "data\airports\$Icao"
$Api = 'https://gateway.x-plane.com/apiv1'
New-Item -ItemType Directory -Force $Out | Out-Null

$id = (Invoke-RestMethod "$Api/airport/$Icao").airport.recommendedSceneryId
if (-not $id) { throw "no recommendedSceneryId for $Icao" }
$blob = (Invoke-RestMethod "$Api/scenery/$id").scenery.masterZipBlob
$zip = New-Object IO.Compression.ZipArchive(
    (New-Object IO.MemoryStream(, [Convert]::FromBase64String($blob))))
try {
    $entry = $zip.Entries | Where-Object { $_.Name -eq "$Icao.dat" } | Select-Object -First 1
    if (-not $entry) { throw "$Icao.dat not in scenery $id" }
    $dest = Join-Path $Out 'apt.dat'
    $in = $entry.Open()
    $file = [IO.File]::Create($dest)
    try { $in.CopyTo($file) } finally { $file.Dispose(); $in.Dispose() }
} finally {
    $zip.Dispose()
}
Write-Host "scenery id: $id"
Write-Host "wrote $dest"
