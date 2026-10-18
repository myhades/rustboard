$ErrorActionPreference = 'Stop'
Push-Location (Join-Path $PSScriptRoot '../../..')
try {
    cargo fmt --all -- --check
    if ($LASTEXITCODE) { throw 'Formatting failed' }
    cargo clippy --all-targets -- -D warnings
    if ($LASTEXITCODE) { throw 'Clippy failed' }
    cargo test --locked
    if ($LASTEXITCODE) { throw 'Tests failed' }
    cargo build --release --locked
    if ($LASTEXITCODE) { throw 'Build failed' }
} finally { Pop-Location }
