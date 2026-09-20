param(
    [Parameter(Mandatory)]
    [string]$Path
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSCommandPath
$target = if ([System.IO.Path]::IsPathRooted($Path)) {
    $Path
} else {
    Join-Path $root $Path
}

if (-not (Test-Path -LiteralPath $target -PathType Leaf)) {
    throw "Nerv source file not found: $Path"
}

if ([System.IO.Path]::GetExtension($target) -ne '.nerv') {
    throw "Nerv source files must use the .nerv extension"
}

Push-Location $root
try {
    & cargo run --quiet -p nerv -- $target --run
    exit $LASTEXITCODE
} finally {
    Pop-Location
}
