$ErrorActionPreference = "Stop"

$FeedNoteRoot = Split-Path -Parent $PSScriptRoot
$InstalledPath = Join-Path $FeedNoteRoot "FeedNote.exe"
$StagedPath = Join-Path $FeedNoteRoot ".tooling\release-ready\FeedNote.exe"
$shell = New-Object -ComObject WScript.Shell

function Test-InstalledFeedNoteRunning {
    $installedFullPath = [IO.Path]::GetFullPath($InstalledPath)
    $processes = @(
        Get-CimInstance Win32_Process -Filter "Name='FeedNote.exe'" -ErrorAction SilentlyContinue |
            Where-Object {
                $_.ExecutablePath -and
                [StringComparer]::OrdinalIgnoreCase.Equals(
                    [IO.Path]::GetFullPath($_.ExecutablePath),
                    $installedFullPath
                )
            }
    )
    return $processes.Count -gt 0
}

function Start-InstalledFeedNote {
    if (-not (Test-InstalledFeedNoteRunning)) {
        Start-Process -FilePath $InstalledPath -WorkingDirectory $FeedNoteRoot -WindowStyle Hidden
    }
}

$updateAvailable = Test-Path -LiteralPath $StagedPath -PathType Leaf
if ($updateAvailable -and (Test-Path -LiteralPath $InstalledPath -PathType Leaf)) {
    $installedHash = (Get-FileHash -LiteralPath $InstalledPath -Algorithm SHA256).Hash
    $stagedHash = (Get-FileHash -LiteralPath $StagedPath -Algorithm SHA256).Hash
    $updateAvailable = $installedHash -ne $stagedHash
}

if ($updateAvailable) {
    $choice = $shell.Popup(
        "发现新的 FeedNote 版本，是否现在更新？`n`n更新完成后 FeedNote 会自动启动。",
        0,
        "FeedNote 更新",
        36
    )
    if ($choice -eq 6) {
        try {
            & (Join-Path $PSScriptRoot "deploy-release.ps1") -SourcePath $StagedPath -NoRestart
        } catch {
            $shell.Popup("FeedNote 更新失败：`n$($_.Exception.Message)", 0, "FeedNote 更新", 16) | Out-Null
        }
    }
}

if (Test-Path -LiteralPath $InstalledPath -PathType Leaf) {
    Start-InstalledFeedNote
} else {
    $shell.Popup("没有找到 FeedNote.exe，请先构建应用。", 0, "FeedNote", 16) | Out-Null
}
