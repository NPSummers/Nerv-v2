$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    . .\setup-llvm.ps1
    & cargo test --workspace --locked @args
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
} finally {
    Pop-Location
}
