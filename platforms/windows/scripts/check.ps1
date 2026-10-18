$ErrorActionPreference = 'Stop'
Push-Location (Join-Path $PSScriptRoot '../../..')
try {
    cargo fmt --all -- --check
    if ($LASTEXITCODE) { throw 'Formatting failed' }
    cargo clippy --workspace --all-targets -- -D warnings
    if ($LASTEXITCODE) { throw 'Clippy failed' }
    cargo test --workspace --locked
    if ($LASTEXITCODE) { throw 'Tests failed' }
    cargo build --workspace --release --locked
    if ($LASTEXITCODE) { throw 'Build failed' }
} finally { Pop-Location }
