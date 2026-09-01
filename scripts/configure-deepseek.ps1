$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$secretsPath = Join-Path $root "data\secrets.env"
$secureKey = Read-Host "Paste the DeepSeek API Key from the DeepSeek console (do not enter this script command)" -AsSecureString
$keyPointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($secureKey)

try {
    $apiKey = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($keyPointer)
    $normalizedKey = $apiKey.Trim()
    $looksLikeCommand = $normalizedKey -match '[\\/]' -or $normalizedKey -match '^\.\\' -or $normalizedKey -match '\.ps1$'
    $hasInvalidCharacters = $normalizedKey -notmatch '^[A-Za-z0-9._-]+$'
    if ([string]::IsNullOrWhiteSpace($normalizedKey) -or $normalizedKey.Length -lt 20 -or $looksLikeCommand -or $hasInvalidCharacters) {
        throw "Invalid DeepSeek API Key. Paste the key generated in the DeepSeek console, not .\scripts\configure-deepseek.ps1."
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
    $updated.Add("DEEPSEEK_API_KEY=$normalizedKey")

    [IO.Directory]::CreateDirectory((Split-Path -Parent $secretsPath)) | Out-Null
    [IO.File]::WriteAllLines($secretsPath, $updated, [Text.UTF8Encoding]::new($false))
    Write-Host "DeepSeek API Key saved to data\secrets.env. The key was not displayed."
} finally {
    if ($keyPointer -ne [IntPtr]::Zero) {
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($keyPointer)
    }
    Remove-Variable apiKey -ErrorAction SilentlyContinue
}
