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
    $listed = winget list --id $id -e --accept-source-agreements 2>$null | Out-String
    if ($listed -match [regex]::Escape($id)) {
        Step "$id is installed"
        continue
    }
    Step "installing $id"
    $wargs = @('install', '--id', $id, '-e', '--silent', '--accept-package-agreements',
        '--accept-source-agreements') + $p[1]
    & winget @wargs
    # 0: installed; -1978335189 (0x8A15002B): no applicable upgrade / already there.
    if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne -1978335189) {
        throw "winget install $id failed with exit code $LASTEXITCODE"
    }
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
rustup default stable-x86_64-pc-windows-msvc
if ($LASTEXITCODE -ne 0) { throw 'rustup failed' }
rustc --version
cargo --version
Write-Host "VULKAN_SDK=$env:VULKAN_SDK"
Write-Host "LIBCLANG_PATH=$env:LIBCLANG_PATH"

if ($NoBuild) { return }
Step 'building (release)'
cargo build --release --locked -p atc-server -p atc-client -p atc-xplane-plugin
if ($LASTEXITCODE -ne 0) { throw "cargo build failed with exit code $LASTEXITCODE" }
Step 'done: target\release\atc-server.exe, atc-client.exe, atc_overlay.dll'
