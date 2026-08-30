$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$secretsPath = Join-Path $root "data\secrets.env"
$appId = (Read-Host "Feishu App ID").Trim()
$secureSecret = Read-Host "Feishu App Secret" -AsSecureString
$secretPointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secureSecret)

try {
    $appSecret = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($secretPointer)
    if (-not $appId.StartsWith("cli_") -or [string]::IsNullOrWhiteSpace($appSecret)) {
        throw "App ID or App Secret is invalid."
    }

    $lines = if (Test-Path -LiteralPath $secretsPath) {
        [Collections.Generic.List[string]](Get-Content -LiteralPath $secretsPath)
    } else {
        [Collections.Generic.List[string]]::new()
    }
    $filtered = $lines | Where-Object {
        $_ -notmatch '^\s*FEISHU_APP_ID=' -and $_ -notmatch '^\s*FEISHU_APP_SECRET='
    }
    $updated = [Collections.Generic.List[string]]::new()
    foreach ($line in $filtered) { $updated.Add($line) }
    if ($updated.Count -gt 0 -and $updated[$updated.Count - 1] -ne "") { $updated.Add("") }
    $updated.Add("FEISHU_APP_ID=$appId")
    $updated.Add("FEISHU_APP_SECRET=$appSecret")

    [IO.Directory]::CreateDirectory((Split-Path -Parent $secretsPath)) | Out-Null
    [IO.File]::WriteAllLines($secretsPath, $updated, [Text.UTF8Encoding]::new($false))
    Write-Host "Feishu credentials saved to data\secrets.env. The secret was not displayed."
} finally {
    if ($secretPointer -ne [IntPtr]::Zero) {
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($secretPointer)
    }
    Remove-Variable appSecret -ErrorAction SilentlyContinue
}
