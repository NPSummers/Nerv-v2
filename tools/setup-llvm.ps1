param([switch]$Minimal)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

function Initialize-LLVM {
    $configs = @(
        $env:LLVM_CONFIG_PATH
        $(if ($env:LLVM_SYS_231_PREFIX) { Join-Path $env:LLVM_SYS_231_PREFIX 'bin\llvm-config.exe' })
        (Join-Path $root 'llvm\bin\llvm-config.exe')
        (Join-Path $root 'clang\bin\llvm-config.exe')
        $(Get-Command llvm-config.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source)
    )
    foreach ($config in $configs) {
        if (-not $config -or -not (Test-Path -LiteralPath $config -PathType Leaf)) { continue }
        $version = & $config --version
        if ($LASTEXITCODE -ne 0 -or $version -notlike '23.1.*') { continue }
        if ($Minimal -and $version -ne '23.1.3') { continue }
        $prefix = Split-Path -Parent (Split-Path -Parent $config)
        if (-not (Test-Path -LiteralPath (Join-Path $prefix 'bin\LLVM-C.dll'))) { continue }
        if (-not (Test-Path -LiteralPath (Join-Path $prefix 'lib\LLVM-C.lib'))) { continue }
        $env:LLVM_CONFIG_PATH = $config
        $env:LLVM_SYS_231_PREFIX = $prefix
        $env:PATH = "$(Join-Path $prefix 'bin');$env:PATH"
        return
    }

    $destination = Join-Path $root 'llvm'
    if (Test-Path -LiteralPath $destination) {
        throw 'llvm/ exists but is not a usable LLVM 23.1 installation. Move it aside and retry.'
    }
    $arch = $env:PROCESSOR_ARCHITEW6432
    if (-not $arch) { $arch = $env:PROCESSOR_ARCHITECTURE }
    switch ($arch) {
        'AMD64' {
            $triple = 'x86_64-pc-windows-msvc'
            $checksum = '9ee6a2192fbb30c776415af529927f76ca6b5d1d55699bb1315f9878e610b5b8'
        }
        'Arm64' {
            $triple = 'aarch64-pc-windows-msvc'
            $checksum = '4a42903db59f990a10c245eb32999f1e3247c37ba66de87f8f8dc2338ed9a79b'
        }
        default { throw "Unsupported Windows architecture: $arch" }
    }
    $version = '23.1.3'
    $archive = "clang+llvm-$version-$triple.tar.xz"
    $sevenZip = Get-Command 7z.exe -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source
    if (-not $sevenZip) {
        $candidate = Join-Path $env:ProgramFiles '7-Zip\7z.exe'
        if (Test-Path -LiteralPath $candidate) { $sevenZip = $candidate }
    }
    $useSevenZip = $Minimal -and $sevenZip
    $extractor = 'Windows tar'
    if ($useSevenZip) {
        $details = & $sevenZip i
        $extractor = ($details | Where-Object { $_ -match '^7-Zip ' } | Select-Object -First 1).Trim()
        if ($arch -eq 'AMD64' -and ($details | Select-String -Pattern '\bzstd\b' -Quiet)) {
            $archive = "clang+llvm-$version-$triple.tar.zst"
            $checksum = '7e69dc07a4536b79ff24a651a2611990bf8eaab17ea560f1092707c4f31ba01d'
        }
    }
    $staging = Join-Path $root ('.llvm-setup.' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $staging | Out-Null
    try {
        Write-Host "Downloading LLVM $version into $destination..."
        $download = Join-Path $staging $archive
        & curl.exe --fail --location --silent --show-error --retry 3 --connect-timeout 30 --max-time 900 --speed-time 60 --speed-limit 1024 --output $download "https://github.com/llvm/llvm-project/releases/download/llvmorg-$version/$archive"
        if ($LASTEXITCODE -ne 0) { throw "LLVM download failed: $LASTEXITCODE" }
        Write-Host 'LLVM download finished; verifying SHA-256...'
        if ((Get-FileHash -LiteralPath $download -Algorithm SHA256).Hash -ne $checksum) {
            throw 'LLVM archive checksum mismatch.'
        }
        $install = Join-Path $staging 'install'
        New-Item -ItemType Directory -Path $install | Out-Null
        Write-Host "Extracting LLVM using $extractor..."
        $patterns = @()
        if ($Minimal) {
            $patterns = @('*/include/*', '*/bin/llvm-config.exe', '*/bin/LLVM-C.dll', '*/lib/LLVM-C.lib', '*/bin/lld.exe', '*/bin/ld.lld.exe', '*/bin/llvm-readobj.exe', '*/bin/llvm-readelf.exe', '*/bin/llvm-objdump.exe', '*/bin/llvm-otool.exe')
        }
        if ($useSevenZip) {
            & $sevenZip x $download "-o$staging\expanded" -y -bso0 -bsp0
            if ($LASTEXITCODE -ne 0) { throw 'Could not decompress the LLVM archive.' }
            $tarFiles = @(Get-ChildItem -LiteralPath (Join-Path $staging 'expanded') -Filter '*.tar' -File)
            if ($tarFiles.Count -ne 1) { throw 'Expected one LLVM tar archive.' }
            $selections = @($patterns | ForEach-Object { "-ir!$_" })
            & $sevenZip x $tarFiles[0].FullName "-o$install" -y -bso0 -bsp0 @selections
            if ($LASTEXITCODE -ne 0) { throw 'Could not extract selected LLVM files.' }
            $directories = @(Get-ChildItem -LiteralPath $install -Directory)
            if ($directories.Count -ne 1) { throw 'Expected one LLVM payload directory.' }
            $install = $directories[0].FullName
        } else {
            $tarArguments = @('-xf', $download, '-C', $install, '--strip-components=1')
            foreach ($pattern in $patterns) {
                $tarArguments += "--include=$pattern"
            }
            & tar @tarArguments
            if ($LASTEXITCODE -ne 0) { throw 'Could not extract the LLVM archive.' }
        }
        Write-Host 'LLVM extraction finished.'
        foreach ($file in @('bin\llvm-config.exe', 'bin\LLVM-C.dll', 'lib\LLVM-C.lib')) {
            if (-not (Test-Path -LiteralPath (Join-Path $install $file))) {
                throw "LLVM archive is missing $file."
            }
        }
        $provenance = @{ version = $version; archive = $archive; sha256 = $checksum; extractor = $extractor } | ConvertTo-Json
        [System.IO.File]::WriteAllText((Join-Path $install 'LLVM-source.json'), $provenance)
        $rootPrefix = [System.IO.Path]::GetFullPath($root).TrimEnd('\') + '\'
        foreach ($path in @($install, $destination)) {
            if (-not [System.IO.Path]::GetFullPath($path).StartsWith($rootPrefix, [StringComparison]::OrdinalIgnoreCase)) {
                throw 'LLVM move escapes the checkout.'
            }
        }
        Move-Item -LiteralPath $install -Destination $destination
    } finally {
        $resolvedStaging = [System.IO.Path]::GetFullPath($staging)
        $expectedPrefix = Join-Path ([System.IO.Path]::GetFullPath($root)) '.llvm-setup.'
        if (-not $resolvedStaging.StartsWith($expectedPrefix, [StringComparison]::OrdinalIgnoreCase)) {
            throw 'Invalid LLVM staging cleanup path.'
        }
        Remove-Item -LiteralPath $resolvedStaging -Recurse -Force
    }
    Initialize-LLVM
}

Initialize-LLVM
