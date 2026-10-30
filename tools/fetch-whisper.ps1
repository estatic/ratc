# Fetch Whisper ggml models into models\whisper\. Idempotent. Windows twin of fetch-whisper.sh.
# Usage: powershell -ExecutionPolicy Bypass -File tools\fetch-whisper.ps1
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$Root = Split-Path -Parent $PSScriptRoot
$Out = Join-Path $Root 'models\whisper'
$HF = 'https://huggingface.co/ggerganov/whisper.cpp/resolve/main'
$Models = @(
    'ggml-large-v3-turbo-q5_0.bin', # primary, multilingual (~574 MB)
    'ggml-small.en-q5_1.bin'        # fallback, English only (~181 MB)
)
New-Item -ItemType Directory -Force $Out | Out-Null
foreach ($m in $Models) {
    $dest = Join-Path $Out $m
    if ((Test-Path $dest) -and (Get-Item $dest).Length -gt 0) {
        Write-Host "$m present, skipping"
        continue
    }
    Write-Host "downloading $m"
    Invoke-WebRequest "$HF/$m" -OutFile "$dest.part" -UseBasicParsing
    Move-Item -Force "$dest.part" $dest
}
Write-Host "done: $Out"
