$FeedNoteRoot = Split-Path -Parent $PSScriptRoot
$env:RUSTUP_HOME = Join-Path $FeedNoteRoot ".tooling\rustup"
$env:CARGO_HOME = Join-Path $FeedNoteRoot ".tooling\cargo"
$env:CARGO_TARGET_DIR = Join-Path $FeedNoteRoot ".tooling\target"
$env:NPM_CONFIG_CACHE = Join-Path $FeedNoteRoot ".tooling\npm-cache"
$env:FEEDNOTE_DATA_DIR = Join-Path $FeedNoteRoot "data"
$env:FEEDNOTE_SECRETS_FILE = Join-Path $env:FEEDNOTE_DATA_DIR "secrets.env"
$env:TEMP = Join-Path $FeedNoteRoot ".tooling\temp"
$env:TMP = $env:TEMP
$env:Path = "$(Join-Path $env:CARGO_HOME 'bin');$env:Path"

New-Item -ItemType Directory -Force -Path $env:RUSTUP_HOME,$env:CARGO_HOME,$env:NPM_CONFIG_CACHE,$env:FEEDNOTE_DATA_DIR,$env:TEMP | Out-Null
