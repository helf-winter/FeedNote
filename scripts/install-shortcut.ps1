$ErrorActionPreference = "Stop"

$FeedNoteRoot = Split-Path -Parent $PSScriptRoot
$LauncherPath = Join-Path $PSScriptRoot "launch-feednote.ps1"
$ManagedPath = Join-Path $FeedNoteRoot ".app\FeedNote.exe"
$InstalledPath = if (Test-Path -LiteralPath $ManagedPath -PathType Leaf) {
    $ManagedPath
} else {
    Join-Path $FeedNoteRoot "FeedNote.exe"
}
$DesktopPath = [Environment]::GetFolderPath("Desktop")
$ExistingPath = Join-Path $DesktopPath "FeedNote.exe.lnk"
$ShortcutPath = if (Test-Path -LiteralPath $ExistingPath) {
    $ExistingPath
} else {
    Join-Path $DesktopPath "FeedNote.lnk"
}
$PowerShellPath = Join-Path $env:WINDIR "System32\WindowsPowerShell\v1.0\powershell.exe"

$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($ShortcutPath)
$shortcut.TargetPath = $PowerShellPath
$shortcut.Arguments = "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$LauncherPath`""
$shortcut.WorkingDirectory = $FeedNoteRoot
$shortcut.IconLocation = "$InstalledPath,0"
$shortcut.Description = "Start FeedNote and check for signed updates"
$shortcut.WindowStyle = 7
$shortcut.Save()

Write-Host "FeedNote desktop launcher installed: $ShortcutPath"
