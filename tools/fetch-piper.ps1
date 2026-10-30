# Fetch the Piper TTS binary and the English and Polish medium voices into models\piper\.
# Idempotent. Windows twin of fetch-piper.sh.
# Usage: powershell -ExecutionPolicy Bypass -File tools\fetch-piper.ps1
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$Root = Split-Path -Parent $PSScriptRoot
$Out = Join-Path $Root 'models\piper'
$Voices = Join-Path $Out 'voices'
$Rel = 'https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_windows_amd64.zip'
$HF = 'https://huggingface.co/rhasspy/piper-voices/resolve/main'
# lang/locale/speaker/quality — see tools/MODELS.md
$VoiceList = @(
    'en/en_US/ryan/medium/en_US-ryan-medium',
    'en/en_GB/alan/medium/en_GB-alan-medium',
    'en/en_US/amy/medium/en_US-amy-medium',
    'en/en_US/lessac/medium/en_US-lessac-medium',
    'en/en_GB/northern_english_male/medium/en_GB-northern_english_male-medium',
    'en/en_US/joe/medium/en_US-joe-medium',
    'pl/pl_PL/darkman/medium/pl_PL-darkman-medium',
    'pl/pl_PL/mc_speech/medium/pl_PL-mc_speech-medium',
    'pl/pl_PL/gosia/medium/pl_PL-gosia-medium'
)
New-Item -ItemType Directory -Force $Out, $Voices | Out-Null

if (-not (Test-Path (Join-Path $Out 'piper\piper.exe'))) {
    $zip = Join-Path ([IO.Path]::GetTempPath()) "piper-$PID.zip"
    try {
        Write-Host 'downloading piper binary'
        Invoke-WebRequest $Rel -OutFile $zip -UseBasicParsing
        Expand-Archive $zip -DestinationPath $Out -Force
    } finally {
        Remove-Item $zip -ErrorAction SilentlyContinue
    }
} else {
    Write-Host 'piper binary present, skipping'
}

foreach ($v in $VoiceList) {
    $name = Split-Path -Leaf $v
    foreach ($ext in 'onnx', 'onnx.json') {
        $dest = Join-Path $Voices "$name.$ext"
        if ((Test-Path $dest) -and (Get-Item $dest).Length -gt 0) {
            Write-Host "$name.$ext present, skipping"
            continue
        }
        Write-Host "downloading $name.$ext"
        Invoke-WebRequest "$HF/$v.$ext" -OutFile "$dest.part" -UseBasicParsing
        Move-Item -Force "$dest.part" $dest
    }
}
Write-Host "done: $Out"
