$ErrorActionPreference = "Stop"

. (Join-Path $PSScriptRoot "env.ps1")
$FeedNoteRoot = Split-Path -Parent $PSScriptRoot
Set-Location $FeedNoteRoot

$SigningKeyPath = Join-Path $FeedNoteRoot "data\updater.key"
$SigningEnvironmentPath = Join-Path $FeedNoteRoot "data\updater-signing.env"
if (-not (Test-Path -LiteralPath $SigningKeyPath -PathType Leaf)) {
    throw "Updater signing key not found: $SigningKeyPath"
}
if (-not (Test-Path -LiteralPath $SigningEnvironmentPath -PathType Leaf)) {
    throw "Updater signing password file not found: $SigningEnvironmentPath"
}

$passwordLine = Get-Content -LiteralPath $SigningEnvironmentPath |
    Where-Object { $_ -match '^TAURI_SIGNING_PRIVATE_KEY_PASSWORD=' } |
    Select-Object -First 1
if (-not $passwordLine) {
    throw "TAURI_SIGNING_PRIVATE_KEY_PASSWORD is missing from $SigningEnvironmentPath"
}

$env:TAURI_SIGNING_PRIVATE_KEY = $SigningKeyPath
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ($passwordLine -split '=', 2)[1]

npm run tauri:build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& (Join-Path $PSScriptRoot "stage-release.ps1")
