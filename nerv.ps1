$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$arguments = @($args)
# Preserve the original named invocation as well as positional source paths.
if ($arguments.Count -gt 0 -and $arguments[0] -eq '-Path') {
    $arguments = @($arguments | Select-Object -Skip 1)
}
if ($arguments.Count -eq 0) {
    throw 'Usage: nerv.ps1 <file.nerv> | build <file.nerv> [-o output] [--debug|--release] [--cpu generic|native] [--emit-object]'
}

$isBuild = $arguments[0] -eq 'build'
if (-not $isBuild) {
    if ($arguments.Count -ne 1) { throw 'Running a source file accepts one path' }
    $path = [string]$arguments[0]
    $target = if ([System.IO.Path]::IsPathRooted($path)) { $path } else { Join-Path $root $path }
    if (-not (Test-Path -LiteralPath $target -PathType Leaf)) { throw "Nerv source file not found: $path" }
    if ([System.IO.Path]::GetExtension($target) -ne '.nerv') { throw 'Nerv source files must use the .nerv extension' }
}

Push-Location $root
try {
    . (Join-Path $root 'tools\setup-llvm.ps1')
    if ($isBuild) { & cargo run --quiet -p nerv -- @arguments }
    else { & cargo run --quiet -p nerv -- $target --run }
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
