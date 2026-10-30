# Install everything needed to build Virtual ATC on Windows 10/11 x86-64 and build it.
# Installs with winget (machine-wide where the package allows): Git, Rust (rustup, MSVC
# toolchain), Visual Studio 2022 Build Tools (C++ workload, includes CMake), LLVM (libclang
# for whisper-rs's bindings) and the Vulkan SDK (Whisper's GPU backend). Already installed
# packages are skipped. Run from the repository root in an elevated PowerShell:
#   powershell -ExecutionPolicy Bypass -File tools\setup-windows.ps1 [-NoBuild]
param([switch]$NoBuild)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

function Step($text) { Write-Host "==> $text" -ForegroundColor Cyan }

# Runs a native program; only its exit code decides success. (Windows PowerShell turns a
# program's stderr lines into errors when output is redirected, and rustup/cargo write their
# progress there.) Codes in -Ok also count as success.
function Native([string]$exe, [string[]]$argv, [int[]]$Ok = @(0)) {
    $ErrorActionPreference = 'Continue'
    & $exe @argv 2>&1 | ForEach-Object {
        if ($_ -is [System.Management.Automation.ErrorRecord]) { $_.Exception.Message } else { "$_" }
    }
    if ($Ok -notcontains $LASTEXITCODE) { throw "$exe $($argv -join ' ') failed with exit code $LASTEXITCODE" }
}

if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
    throw 'winget not found: install "App Installer" from the Microsoft Store, then run again'
}

# id, extra winget arguments
$packages = @(
    @('Git.Git', @()),
    @('Rustlang.Rustup', @()),
    @('Microsoft.VisualStudio.2022.BuildTools', @('--override',
        '--quiet --wait --norestart --nocache --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended')),
    @('LLVM.LLVM', @()),
    @('KhronosGroup.VulkanSDK', @())
)
foreach ($p in $packages) {
    $id = $p[0]
    $listed = & { $ErrorActionPreference = 'Continue'
        winget list --id $id -e --accept-source-agreements 2>$null | Out-String }
    if ($listed -match [regex]::Escape($id)) {
        Step "$id is installed"
        continue
    }
    Step "installing $id"
    $wargs = @('install', '--id', $id, '-e', '--silent', '--accept-package-agreements',
        '--accept-source-agreements') + $p[1]
    # -1978335189 (0x8A15002B): no applicable upgrade, i.e. already there.
    Native winget $wargs -Ok 0, -1978335189
}

# This session does not see the PATH and variables the installers just set.
Step 'refreshing the environment'
foreach ($scope in 'Machine', 'User') {
    foreach ($kv in [Environment]::GetEnvironmentVariables($scope).GetEnumerator()) {
        if ($kv.Key -ne 'Path') { Set-Item "env:$($kv.Key)" $kv.Value }
    }
}
$env:Path = [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' +
    [Environment]::GetEnvironmentVariable('Path', 'User') + ";$env:USERPROFILE\.cargo\bin"

# CMake comes with the Build Tools but is on PATH only in a Developer Prompt.
if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    $vs = & $vswhere -latest -products * -property installationPath
    $cmake = Join-Path $vs 'Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin'
    if (-not (Test-Path (Join-Path $cmake 'cmake.exe'))) { throw "CMake not found under $vs" }
    $env:Path += ";$cmake"
}

if (-not $env:VULKAN_SDK) {
    $sdk = Get-ChildItem 'C:\VulkanSDK' -Directory -ErrorAction SilentlyContinue |
        Sort-Object Name | Select-Object -Last 1
    if ($sdk) { $env:VULKAN_SDK = $sdk.FullName }
}
if (-not $env:VULKAN_SDK) { throw 'Vulkan SDK not found (VULKAN_SDK is not set)' }
if (-not $env:LIBCLANG_PATH) {
    $llvm = Join-Path $env:ProgramFiles 'LLVM\bin'
    if (Test-Path (Join-Path $llvm 'libclang.dll')) {
        $env:LIBCLANG_PATH = $llvm
        [Environment]::SetEnvironmentVariable('LIBCLANG_PATH', $llvm, 'User')
    }
}

Step 'Rust toolchain'
Native rustup @('default', 'stable-x86_64-pc-windows-msvc')
Native rustc @('--version')
Native cargo @('--version')
Write-Host "VULKAN_SDK=$env:VULKAN_SDK"
Write-Host "LIBCLANG_PATH=$env:LIBCLANG_PATH"
Native cmake @('--version')

if ($NoBuild) { return }
Step 'building (release)'
Native cargo @('build', '--release', '--locked', '-p', 'atc-server', '-p', 'atc-client',
    '-p', 'atc-xplane-plugin')
Step 'done: target\release\atc-server.exe, atc-client.exe, atc_overlay.dll'
