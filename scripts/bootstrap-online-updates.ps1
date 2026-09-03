param(
    [string]$InstallerPath
)

$ErrorActionPreference = "Stop"

. (Join-Path $PSScriptRoot "env.ps1")
$FeedNoteRoot = Split-Path -Parent $PSScriptRoot
$ManagedDirectory = Join-Path $FeedNoteRoot ".app"
$ManagedPath = Join-Path $ManagedDirectory "FeedNote.exe"

if ([string]::IsNullOrWhiteSpace($InstallerPath)) {
    $InstallerPath = Get-ChildItem `
        -LiteralPath (Join-Path $env:CARGO_TARGET_DIR "release\bundle\nsis") `
        -Filter "FeedNote_*-setup.exe" `
        -File |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1 -ExpandProperty FullName
}

if (-not $InstallerPath -or -not (Test-Path -LiteralPath $InstallerPath -PathType Leaf)) {
    throw "FeedNote NSIS installer not found. Run scripts\build.ps1 first."
}

$pathsToStop = @(
    (Join-Path $FeedNoteRoot "FeedNote.exe"),
    $ManagedPath
) | ForEach-Object { [IO.Path]::GetFullPath($_) }

$runningProcesses = @(
    Get-CimInstance Win32_Process -Filter "Name='FeedNote.exe'" -ErrorAction SilentlyContinue |
        Where-Object {
            $_.ExecutablePath -and
            $pathsToStop -contains [IO.Path]::GetFullPath($_.ExecutablePath)
        }
)
foreach ($process in $runningProcesses) {
    Stop-Process -Id $process.ProcessId -Force -ErrorAction Stop
    Wait-Process -Id $process.ProcessId -Timeout 15 -ErrorAction SilentlyContinue
}

New-Item -ItemType Directory -Force -Path $ManagedDirectory | Out-Null
$installer = Start-Process `
    -FilePath ([IO.Path]::GetFullPath($InstallerPath)) `
    -ArgumentList "/S", "/NS", "/D=$ManagedDirectory" `
    -WindowStyle Hidden `
    -Wait `
    -PassThru
if ($installer.ExitCode -ne 0) {
    throw "FeedNote installer exited with code $($installer.ExitCode)."
}
if (-not (Test-Path -LiteralPath $ManagedPath -PathType Leaf)) {
    throw "FeedNote installer did not create the managed executable: $ManagedPath"
}

& (Join-Path $PSScriptRoot "install-shortcut.ps1")
& (Join-Path $PSScriptRoot "launch-feednote.ps1")

Write-Host "FeedNote online-update bootstrap completed: $ManagedPath"
