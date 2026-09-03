$ErrorActionPreference = "Stop"

$FeedNoteRoot = Split-Path -Parent $PSScriptRoot
$ManagedPath = Join-Path $FeedNoteRoot ".app\FeedNote.exe"
$LegacyPath = Join-Path $FeedNoteRoot "FeedNote.exe"
$InstalledPath = if (Test-Path -LiteralPath $ManagedPath -PathType Leaf) {
    $ManagedPath
} else {
    $LegacyPath
}
$StagedPath = Join-Path $FeedNoteRoot ".tooling\release-ready\FeedNote.exe"
$shell = New-Object -ComObject WScript.Shell
$env:FEEDNOTE_DATA_DIR = Join-Path $FeedNoteRoot "data"

function ConvertFrom-Utf8Base64([string]$Value) {
    return [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String($Value))
}

function Start-InstalledFeedNote {
    Start-Process `
        -FilePath $InstalledPath `
        -ArgumentList "--check-updates" `
        -WorkingDirectory $FeedNoteRoot `
        -WindowStyle Hidden
}

$updateAvailable = Test-Path -LiteralPath $StagedPath -PathType Leaf
if ($updateAvailable -and (Test-Path -LiteralPath $InstalledPath -PathType Leaf)) {
    $installedHash = (Get-FileHash -LiteralPath $InstalledPath -Algorithm SHA256).Hash
    $stagedHash = (Get-FileHash -LiteralPath $StagedPath -Algorithm SHA256).Hash
    $updateAvailable = $installedHash -ne $stagedHash
}

if ($updateAvailable) {
    $updateTitle = ConvertFrom-Utf8Base64 "RmVlZE5vdGUg5pu05paw"
    $updatePrompt =
        (ConvertFrom-Utf8Base64 "5Y+R546w5paw55qEIEZlZWROb3RlIOeJiOacrO+8jOaYr+WQpueOsOWcqOabtOaWsO+8nw==") +
        [Environment]::NewLine + [Environment]::NewLine +
        (ConvertFrom-Utf8Base64 "5pu05paw5a6M5oiQ5ZCOIEZlZWROb3RlIOS8muiHquWKqOWQr+WKqOOAgg==")
    $choice = $shell.Popup(
        $updatePrompt,
        0,
        $updateTitle,
        36
    )
    if ($choice -eq 6) {
        try {
            & (Join-Path $PSScriptRoot "deploy-release.ps1") `
                -SourcePath $StagedPath `
                -InstalledPath $InstalledPath `
                -NoRestart
        } catch {
            $errorMessage =
                (ConvertFrom-Utf8Base64 "RmVlZE5vdGUg5pu05paw5aSx6LSl77ya") +
                [Environment]::NewLine + $_.Exception.Message
            $shell.Popup($errorMessage, 0, $updateTitle, 16) | Out-Null
        }
    }
}

if (Test-Path -LiteralPath $InstalledPath -PathType Leaf) {
    Start-InstalledFeedNote
} else {
    $missingMessage = ConvertFrom-Utf8Base64 "5rKh5pyJ5om+5YiwIEZlZWROb3RlLmV4Ze+8jOivt+WFiOaehOW7uuW6lOeUqOOAgg=="
    $shell.Popup($missingMessage, 0, "FeedNote", 16) | Out-Null
}
