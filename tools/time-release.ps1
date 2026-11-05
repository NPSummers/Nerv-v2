param(
    [string]$ProgramPath = '.\example\heavy.nerv',
    [int]$Runs = 5,
    [int]$Warmups = 1,
    [ValidateSet('generic', 'native')][string]$Cpu = 'generic',
    [switch]$SkipBuild,
    [switch]$ShowProgramOutput,
    [switch]$SkipRustBaseline,
    [string]$NervExePath = '.\target\release\heavy-nerv.exe',
    [string]$RustExePath = '.\time-release\target\release\heavy.exe',
    [string]$JsonOutput
)

$ErrorActionPreference = 'Stop'
$scriptRoot = Split-Path -Parent $PSScriptRoot

function Resolve-LocalPath([string]$Path) {
    if (-not [System.IO.Path]::IsPathRooted($Path)) { $Path = Join-Path $scriptRoot $Path }
    return [System.IO.Path]::GetFullPath($Path)
}

function Measure-One([string]$Label, [string]$Executable, [bool]$ShowOutput) {
    $timer = [System.Diagnostics.Stopwatch]::StartNew()
    $output = @(& $Executable 2>&1)
    $code = $LASTEXITCODE
    $timer.Stop()
    if ($code -ne 0) { throw "$Label failed with exit code ${code}: $($output -join [Environment]::NewLine)" }
    if ($script:isHeavy) {
        $expectedLines = if ($Label -eq 'Nerv') { 2 } else { 1 }
        if ($output.Count -ne $expectedLines) { throw "$Label produced unexpected benchmark output: $output" }
        if ($Label -eq 'Nerv' -and [string]$output[1] -ne '0') { throw 'Nerv void-main result must be 0' }
        $value = [long]::Parse([string]$output[0])
        if ($value -lt 1049940 -or $value -ge 1059940) { throw "$Label produced an incorrect benchmark result: $value" }
    }
    if ($ShowOutput) { Write-Host ("  {0} output:`n{1}" -f $Label, ($output -join [Environment]::NewLine)) }
    return $timer.Elapsed.TotalMilliseconds
}

function Get-Statistics([double[]]$Values) {
    $sorted = @($Values | Sort-Object)
    $middle = [int][Math]::Floor($sorted.Count / 2)
    $median = if ($sorted.Count % 2) { $sorted[$middle] } else { ($sorted[$middle - 1] + $sorted[$middle]) / 2 }
    return [ordered]@{
        samples_ms = @($Values)
        min_ms = $sorted[0]
        median_ms = $median
        mean_ms = ($Values | Measure-Object -Average).Average
        max_ms = $sorted[-1]
    }
}

Push-Location $scriptRoot
try {
    if ($Runs -lt 1 -or $Warmups -lt 0) { throw 'Runs must be >= 1 and Warmups must be >= 0' }
    $program = Resolve-LocalPath $ProgramPath
    $nervExe = Resolve-LocalPath $NervExePath
    $rustExe = Resolve-LocalPath $RustExePath
    $compiler = Resolve-LocalPath '.\target\release\nerv.exe'
    $rustcVersion = $null
    $llvmVersion = $null
    if ($nervExe -eq $compiler -or $nervExe -eq $rustExe -or $nervExe -eq $program) {
        throw 'The standalone Nerv output must differ from the compiler, source, and Rust executable'
    }
    if ($JsonOutput -and (Resolve-LocalPath $JsonOutput) -in @($nervExe, $rustExe, $compiler, $program)) {
        throw 'JSON output must not overwrite a source file, compiler, or timed executable'
    }
    if (-not $SkipBuild) {
        if (-not (Test-Path -LiteralPath $program -PathType Leaf)) { throw "Program not found: $program" }
        . (Join-Path $scriptRoot 'tools\setup-llvm.ps1')
        $rustcPath = if ($env:RUSTC) { $env:RUSTC } else { 'rustc' }
        $rustcVersion = (& $rustcPath --version) -join ' '
        $llvmVersion = (& $env:LLVM_CONFIG_PATH --version) -join ' '
        Write-Host 'Building the Nerv compiler (not timed)...'
        & cargo build --quiet --release --locked -p nerv --target-dir (Join-Path $scriptRoot 'target')
        if ($LASTEXITCODE -ne 0) { throw 'Nerv compiler build failed' }
        Write-Host 'Compiling the standalone Nerv benchmark (--release, not timed)...'
        & $compiler build $program -o $nervExe --release --cpu $Cpu
        if ($LASTEXITCODE -ne 0) { throw 'Standalone Nerv build failed' }
        if (-not $SkipRustBaseline) {
            Write-Host 'Building release (Rust baseline, not timed)...'
            if ($env:CARGO_ENCODED_RUSTFLAGS) { throw 'Unset CARGO_ENCODED_RUSTFLAGS for matched CPU benchmarking' }
            $originalFlags = $env:RUSTFLAGS
            $effectiveRustFlags = "$originalFlags -C target-cpu=$Cpu"
            try {
                $env:RUSTFLAGS = $effectiveRustFlags
                & cargo build --quiet --release --locked --manifest-path (Join-Path $scriptRoot 'time-release\Cargo.toml') --target-dir (Join-Path $scriptRoot 'time-release\target')
                if ($LASTEXITCODE -ne 0) { throw 'Rust baseline build failed' }
            } finally { $env:RUSTFLAGS = $originalFlags }
        }
    }
    $binaries = [ordered]@{ Nerv = $nervExe }
    if (-not $SkipRustBaseline) { $binaries['Rust'] = $rustExe }
    foreach ($path in $binaries.Values) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Executable not found: $path" }
    }
    $script:isHeavy = $program -eq (Resolve-LocalPath '.\example\heavy.nerv')
    $workload = if ($script:isHeavy) { '10,000 x 100,000 = 1,000,000,000 logical inner iterations' } else { 'custom program' }
    Write-Host "Workload: $workload"
    Write-Host 'Timing standalone release programs; compilation excluded.'
    foreach ($label in $binaries.Keys) { Write-Host ("{0}: {1}" -f $label, $binaries[$label]) }

    # Only builds need LLVM. The measured executables run without its environment.
    $saved = @{}
    foreach ($key in @('LLVM_SYS_231_PREFIX', 'LLVM_CONFIG_PATH', 'DYLD_LIBRARY_PATH', 'DYLD_FALLBACK_LIBRARY_PATH', 'LD_LIBRARY_PATH')) {
        $saved[$key] = [Environment]::GetEnvironmentVariable($key, 'Process')
        [Environment]::SetEnvironmentVariable($key, $null, 'Process')
    }
    try {
        for ($warmup = 1; $warmup -le $Warmups; $warmup++) {
            foreach ($label in $binaries.Keys) { $null = Measure-One $label ($binaries[$label]) $false }
            Write-Host "Warmup $warmup/$Warmups complete (not scored)."
        }
        $samples = @{}
        foreach ($label in $binaries.Keys) { $samples[$label] = [System.Collections.Generic.List[double]]::new() }
        for ($run = 1; $run -le $Runs; $run++) {
            $order = @($binaries.Keys)
            if ($run % 2 -eq 0) { [array]::Reverse($order) }
            Write-Host "`nRound $run/${Runs}:"
            foreach ($label in $order) {
                $elapsed = Measure-One $label ($binaries[$label]) ($ShowProgramOutput.IsPresent)
                $samples[$label].Add($elapsed)
                Write-Host ("  {0}: {1:F3} ms ({2:F6} s)" -f $label, $elapsed, ($elapsed / 1000))
            }
        }
    } finally {
        foreach ($key in $saved.Keys) { [Environment]::SetEnvironmentVariable($key, $saved[$key], 'Process') }
    }
    $measurements = [ordered]@{}
    Write-Host "`n=== Standalone release comparison ==="
    foreach ($label in $binaries.Keys) {
        $s = Get-Statistics ($samples[$label].ToArray())
        $measurements[$label] = $s
        Write-Host ("{0}: median {1:F3} ms | avg {2:F3} ms | min {3:F3} ms | max {4:F3} ms" -f $label, $s.median_ms, $s.mean_ms, $s.min_ms, $s.max_ms)
    }
    $ratio = $null
    if (-not $SkipRustBaseline) {
        $ratio = $measurements.Nerv.median_ms / $measurements.Rust.median_ms
        $comparison = if ($ratio -ge 1) { '{0:F2}x slower' -f $ratio } else { '{0:F2}x faster' -f (1 / $ratio) }
        Write-Host ("Nerv / Rust median ratio: {0:F3}x ({1})" -f $ratio, $comparison)
    }
    if ($JsonOutput) {
        $path = Resolve-LocalPath $JsonOutput
        [System.IO.Directory]::CreateDirectory((Split-Path -Parent $path)) | Out-Null
        $report = [ordered]@{
            timestamp_utc = (Get-Date).ToUniversalTime().ToString('o')
            platform = [System.Runtime.InteropServices.RuntimeInformation]::OSDescription
            architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
            cpu = $env:PROCESSOR_IDENTIFIER; rustc_version = $rustcVersion
            llvm_version = $llvmVersion; rustflags = $env:RUSTFLAGS
            cpu_target = $(if ($SkipBuild) { $null } else { $Cpu })
            effective_rustflags = $effectiveRustFlags
            workload = $workload; runs = $Runs; warmups = $Warmups
            execution = 'standalone'; build_mode = 'release'; build_time_included = $false
            binaries = $binaries; measurements = $measurements; nerv_over_rust_median = $ratio
        }
        [System.IO.File]::WriteAllText($path, (($report | ConvertTo-Json -Depth 8) + [Environment]::NewLine), [System.Text.UTF8Encoding]::new($false))
        Write-Host "Saved report: $path"
    }
} finally {
    Pop-Location
}
