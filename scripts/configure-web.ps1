param(
    [string]$BaseToken = "",
    [string]$TableId = "",
    [string]$AllowedOpenIds = "",
    [string]$RedirectUri = "http://127.0.0.1:4174/api/auth/callback",
    [string]$WebOrigin = "http://127.0.0.1:4173"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$secretsPath = Join-Path $root "data\secrets.env"

if (-not (Test-Path $secretsPath)) {
    throw "Missing data\secrets.env. Configure Feishu credentials first."
}

if ([string]::IsNullOrWhiteSpace($BaseToken)) {
    $BaseToken = Read-Host "Feishu Base token"
}
if ([string]::IsNullOrWhiteSpace($TableId)) {
    $TableId = Read-Host "Feishu Base table id"
}
if ([string]::IsNullOrWhiteSpace($AllowedOpenIds)) {
    $AllowedOpenIds = Read-Host "Allowed Feishu open_id values (comma-separated)"
}
if ([string]::IsNullOrWhiteSpace($BaseToken) -or
    [string]::IsNullOrWhiteSpace($TableId) -or
    [string]::IsNullOrWhiteSpace($AllowedOpenIds)) {
    throw "Base token, table id, and at least one allowed open_id are required."
}

$values = [ordered]@{
    FEISHU_BASE_APP_TOKEN = $BaseToken
    FEISHU_BASE_TABLE_ID = $TableId
    FEISHU_WEB_ALLOWED_OPEN_IDS = $AllowedOpenIds
    FEISHU_WEB_REDIRECT_URI = $RedirectUri
    FEISHU_WEB_ORIGIN = $WebOrigin
    FEISHU_WEB_PORT = "4174"
}
$keys = $values.Keys | ForEach-Object { [regex]::Escape($_) }
$pattern = "^\s*(" + ($keys -join "|") + ")="
$lines = @(Get-Content $secretsPath | Where-Object { $_ -notmatch $pattern })
foreach ($entry in $values.GetEnumerator()) {
    $lines += "$($entry.Key)=$($entry.Value)"
}
[IO.File]::WriteAllLines($secretsPath, $lines, [Text.UTF8Encoding]::new($false))
Write-Host "FeedNote web configuration saved. No secret value was displayed."
