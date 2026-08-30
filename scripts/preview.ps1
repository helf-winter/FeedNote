. (Join-Path $PSScriptRoot "env.ps1")
Set-Location (Split-Path -Parent $PSScriptRoot)
npm run dev
