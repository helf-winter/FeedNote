$ErrorActionPreference = "Stop"

. (Join-Path $PSScriptRoot "env.ps1")
$FeedNoteRoot = Split-Path -Parent $PSScriptRoot
Set-Location $FeedNoteRoot

npm run tauri:build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& (Join-Path $PSScriptRoot "stage-release.ps1")
