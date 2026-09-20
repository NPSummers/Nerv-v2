param(
    [string]$ProgramPath = ".\example\heavy.nerv",
    [int]$Runs = 5,
    [switch]$SkipBuild,
    [switch]$ShowProgramOutput,
    [switch]$SkipRustBaseline,
    [string]$NervExePath = ".\target\release\nerv.exe",
    [string]$RustExePath = ".\old\rust-test\target\release\heavy.exe"
)

$ErrorActionPreference = 'Stop'
$scriptRoot = Split-Path -Parent $PSCommandPath

function Resolve-LocalPath([string]$Path) {
    if ([System.IO.Path]::IsPathRooted($Path)) {
        return $Path
    }
    return Join-Path $scriptRoot $Path
}

function Measure-Runs([string]$Label, [scriptblock]$Command) {
    Write-Host ""
    Write-Host "=== $Label ==="
    $times = [System.Collections.Generic.List[double]]::new()

    for ($run = 1; $run -le $Runs; $run++) {
        $stopwatch = [System.Diagnostics.Stopwatch]::StartNew()
        & $Command
        if ($LASTEXITCODE -ne 0) {
            throw "$Label failed with exit code $LASTEXITCODE."
        }
        $stopwatch.Stop()
        $times.Add($stopwatch.Elapsed.TotalMilliseconds)
        Write-Host ("  Run {0}: {1:N3} ms ({2:N6} s)" -f $run, $stopwatch.Elapsed.TotalMilliseconds, $stopwatch.Elapsed.TotalSeconds)
    }

    $average = ($times | Measure-Object -Average).Average
    $minimum = ($times | Measure-Object -Minimum).Minimum
    $maximum = ($times | Measure-Object -Maximum).Maximum
    Write-Host ("  Min: {0:N3} ms  Avg: {1:N3} ms  Max: {2:N3} ms" -f $minimum, $average, $maximum)
    return $average
}

Push-Location $scriptRoot
try {
    if ($Runs -lt 1) {
        throw 'Runs must be >= 1.'
    }

    $program = Resolve-LocalPath $ProgramPath
    $nervExe = Resolve-LocalPath $NervExePath
    $rustExe = Resolve-LocalPath $RustExePath

    if (-not (Test-Path -LiteralPath $program -PathType Leaf)) {
        throw "Program file not found: $ProgramPath"
    }

    if (-not $SkipBuild) {
        Write-Host 'Building release (Nerv)...'
        & cargo build --release -p nerv | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw 'Nerv release build failed.'
        }

        if (-not $SkipRustBaseline) {
            Write-Host 'Building release (Rust baseline)...'
            Push-Location '.\old\rust-test'
            try {
                & cargo build --release | Out-Null
                if ($LASTEXITCODE -ne 0) {
                    throw 'Rust baseline release build failed.'
                }
            } finally {
                Pop-Location
            }
        }
    }

    if (-not (Test-Path -LiteralPath $nervExe -PathType Leaf)) {
        throw "Nerv release executable not found: $NervExePath"
    }

    $nervCommand = if ($ShowProgramOutput) {
        { & $nervExe $program --run }
    } else {
        { & $nervExe $program --run | Out-Null }
    }
    $nervAverage = Measure-Runs "Nerv ($NervExePath $ProgramPath)" $nervCommand

    if ($SkipRustBaseline) {
        exit 0
    }
    if (-not (Test-Path -LiteralPath $rustExe -PathType Leaf)) {
        throw "Rust baseline executable not found: $RustExePath"
    }

    $rustCommand = if ($ShowProgramOutput) {
        { & $rustExe }
    } else {
        { & $rustExe | Out-Null }
    }
    $rustAverage = Measure-Runs "Rust ($RustExePath)" $rustCommand

    Write-Host ""
    Write-Host "=== Comparison (avg over $Runs runs) ==="
    Write-Host ("  Nerv : {0:N3} ms" -f $nervAverage)
    Write-Host ("  Rust : {0:N3} ms" -f $rustAverage)
    if ($rustAverage -gt 0) {
        $ratio = $nervAverage / $rustAverage
        if ($ratio -ge 1) {
            Write-Host ("  Ratio: {0:N2}x slower (Nerv / Rust)" -f $ratio)
        } else {
            Write-Host ("  Ratio: {0:N2}x faster (Rust / Nerv)" -f (1 / $ratio))
        }
    }
} finally {
    Pop-Location
}
