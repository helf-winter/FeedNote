$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$secretsPath = Join-Path $root "data\secrets.env"
$secureKey = Read-Host "DeepSeek API Key" -AsSecureString
$keyPointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secureKey)

try {
    $apiKey = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($keyPointer)
    if ([string]::IsNullOrWhiteSpace($apiKey) -or $apiKey.Trim().Length -lt 10) {
        throw "DeepSeek API Key is invalid."
    }

    $lines = if (Test-Path -LiteralPath $secretsPath) {
        [Collections.Generic.List[string]](Get-Content -LiteralPath $secretsPath)
    } else {
        [Collections.Generic.List[string]]::new()
    }
    $filtered = $lines | Where-Object { $_ -notmatch '^\s*DEEPSEEK_API_KEY=' }
    $updated = [Collections.Generic.List[string]]::new()
    foreach ($line in $filtered) { $updated.Add($line) }
    if ($updated.Count -gt 0 -and $updated[$updated.Count - 1] -ne "") { $updated.Add("") }
    $updated.Add("DEEPSEEK_API_KEY=$($apiKey.Trim())")

    [IO.Directory]::CreateDirectory((Split-Path -Parent $secretsPath)) | Out-Null
    [IO.File]::WriteAllLines($secretsPath, $updated, [Text.UTF8Encoding]::new($false))
    Write-Host "DeepSeek API Key saved to data\secrets.env. The key was not displayed."
} finally {
    if ($keyPointer -ne [IntPtr]::Zero) {
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($keyPointer)
    }
    Remove-Variable apiKey -ErrorAction SilentlyContinue
}
