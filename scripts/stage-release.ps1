param(
    [string]$SourcePath
)

$ErrorActionPreference = "Stop"

. (Join-Path $PSScriptRoot "env.ps1")
$FeedNoteRoot = Split-Path -Parent $PSScriptRoot
$ReleaseDirectory = Join-Path $FeedNoteRoot ".tooling\release-ready"
$StagedPath = Join-Path $ReleaseDirectory "FeedNote.exe"

if ([string]::IsNullOrWhiteSpace($SourcePath)) {
    $SourcePath = Join-Path $env:CARGO_TARGET_DIR "release\feednote.exe"
}
$SourcePath = [IO.Path]::GetFullPath($SourcePath)

if (-not (Test-Path -LiteralPath $SourcePath -PathType Leaf)) {
    throw "Release executable not found: $SourcePath"
}

New-Item -ItemType Directory -Force -Path $ReleaseDirectory | Out-Null
Copy-Item -LiteralPath $SourcePath -Destination $StagedPath -Force

$sourceHash = (Get-FileHash -LiteralPath $SourcePath -Algorithm SHA256).Hash
$stagedHash = (Get-FileHash -LiteralPath $StagedPath -Algorithm SHA256).Hash
if ($sourceHash -ne $stagedHash) {
    throw "Staged executable hash does not match the release build."
}

Write-Host "FeedNote update staged and verified: $StagedPath"
Write-Host "The desktop launcher will offer this update the next time it is opened."
