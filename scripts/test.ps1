. (Join-Path $PSScriptRoot "env.ps1")
$FeedNoteRoot = Split-Path -Parent $PSScriptRoot
Set-Location $FeedNoteRoot

npm test
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

npm run build
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& (Join-Path $env:CARGO_HOME "bin\cargo.exe") test --manifest-path (Join-Path $FeedNoteRoot "src-tauri\Cargo.toml")
exit $LASTEXITCODE
