$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$arguments = @($args)
if ($arguments.Count -gt 0 -and $arguments[0] -eq '-Path') {
    $arguments = @($arguments | Select-Object -Skip 1)
}
if ($arguments.Count -eq 0) { $arguments = @('--help') }
$commands = @('new', 'run', 'build', 'check', 'test', 'fmt', '--help', '-h', '--version', '-V')
if ($arguments[0] -notin $commands) {
    $path = [string]$arguments[0]
    if (-not (Test-Path -LiteralPath $path) -and -not [System.IO.Path]::IsPathRooted($path)) { $path = Join-Path $root $path }
    $arguments[0] = $path
    if ($arguments.Count -eq 1) { $arguments += '--run' }
}
if ($arguments[0] -eq 'build' -and $arguments.Count -gt 1) {
    $source = [string]$arguments[1]
    if ([System.IO.Path]::GetExtension($source) -eq '.nerv' -and -not (Test-Path -LiteralPath $source) -and -not [System.IO.Path]::IsPathRooted($source)) {
        $candidate = Join-Path $root $source
        if (Test-Path -LiteralPath $candidate) { $arguments[1] = $candidate }
    }
}
. (Join-Path $root 'tools\setup-llvm.ps1')
& cargo run --quiet --locked --manifest-path (Join-Path $root 'Cargo.toml') -p nerv -- @arguments
exit $LASTEXITCODE
