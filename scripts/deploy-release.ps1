param(
    [string]$SourcePath,
    [switch]$NoRestart
)

$ErrorActionPreference = "Stop"

. (Join-Path $PSScriptRoot "env.ps1")
$FeedNoteRoot = Split-Path -Parent $PSScriptRoot
$InstalledPath = Join-Path $FeedNoteRoot "FeedNote.exe"
$ReleaseDirectory = Join-Path $FeedNoteRoot ".tooling\release-ready"
$StagedPath = Join-Path $ReleaseDirectory "FeedNote.exe"
$BackupPath = Join-Path $ReleaseDirectory "FeedNote.previous.exe"

if ([string]::IsNullOrWhiteSpace($SourcePath)) {
    $SourcePath = Join-Path $env:CARGO_TARGET_DIR "release\feednote.exe"
}
$SourcePath = [IO.Path]::GetFullPath($SourcePath)

if (-not (Test-Path -LiteralPath $SourcePath -PathType Leaf)) {
    throw "Release executable not found: $SourcePath"
}

New-Item -ItemType Directory -Force -Path $ReleaseDirectory | Out-Null
if (-not [StringComparer]::OrdinalIgnoreCase.Equals($SourcePath, $StagedPath)) {
    Copy-Item -LiteralPath $SourcePath -Destination $StagedPath -Force
}

$sourceHash = (Get-FileHash -LiteralPath $SourcePath -Algorithm SHA256).Hash
$stagedHash = (Get-FileHash -LiteralPath $StagedPath -Algorithm SHA256).Hash
if ($sourceHash -ne $stagedHash) {
    throw "Staged executable hash does not match the release build."
}

$installedFullPath = [IO.Path]::GetFullPath($InstalledPath)
$runningProcesses = @(
    Get-CimInstance Win32_Process -Filter "Name='FeedNote.exe'" -ErrorAction SilentlyContinue |
        Where-Object {
            $_.ExecutablePath -and
            [StringComparer]::OrdinalIgnoreCase.Equals(
                [IO.Path]::GetFullPath($_.ExecutablePath),
                $installedFullPath
            )
        }
)

foreach ($process in $runningProcesses) {
    Stop-Process -Id $process.ProcessId -Force -ErrorAction Stop
    Wait-Process -Id $process.ProcessId -Timeout 15 -ErrorAction SilentlyContinue
}

$hadPreviousVersion = Test-Path -LiteralPath $InstalledPath -PathType Leaf
$deploySucceeded = $false
try {
    if ($hadPreviousVersion) {
        Copy-Item -LiteralPath $InstalledPath -Destination $BackupPath -Force
    }
    Copy-Item -LiteralPath $StagedPath -Destination $InstalledPath -Force

    $installedHash = (Get-FileHash -LiteralPath $InstalledPath -Algorithm SHA256).Hash
    if ($installedHash -ne $stagedHash) {
        throw "Installed executable hash does not match the staged release."
    }
    $deploySucceeded = $true
} catch {
    if ($hadPreviousVersion -and (Test-Path -LiteralPath $BackupPath -PathType Leaf)) {
        Copy-Item -LiteralPath $BackupPath -Destination $InstalledPath -Force
    }
    throw
} finally {
    if (-not $NoRestart -and (Test-Path -LiteralPath $InstalledPath -PathType Leaf)) {
        Start-Process -FilePath $InstalledPath -WorkingDirectory $FeedNoteRoot -WindowStyle Hidden
    }
}

if ($deploySucceeded) {
    Write-Host "FeedNote release installed and verified: $InstalledPath"
    if ($NoRestart) {
        Write-Host "FeedNote was not restarted because -NoRestart was specified."
    } else {
        Write-Host "FeedNote restarted. The desktop shortcut now opens this release."
    }
}
