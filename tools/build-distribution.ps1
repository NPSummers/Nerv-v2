$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location -LiteralPath $root
function Invoke-Checked {
    param([string]$Program, [string[]]$Arguments)
    $previousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        & $Program @Arguments
        $commandExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previousPreference
    }
    if ($commandExitCode -ne 0) { throw "$Program failed with exit code $commandExitCode" }
}
$target = 'x86_64-pc-windows-gnullvm'
$version = (python tools/release_version.py | ConvertFrom-Json).version
if ($LASTEXITCODE -ne 0) { throw 'Release version validation failed' }
. "$PSScriptRoot\setup-llvm.ps1" -Minimal
Invoke-Checked python @('tools/setup-toolchain.py', 'windows')
$mingwRoot = (Resolve-Path 'target/toolchains/llvm-mingw').Path
$env:CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER = Join-Path $mingwRoot 'bin/x86_64-w64-mingw32-clang.exe'
$env:CC_x86_64_pc_windows_gnullvm = $env:CARGO_TARGET_X86_64_PC_WINDOWS_GNULLVM_LINKER
$env:AR_x86_64_pc_windows_gnullvm = Join-Path $mingwRoot 'bin/llvm-ar.exe'
$env:PATH = "$mingwRoot\bin;$env:PATH"
Invoke-Checked rustup @('target', 'add', $target)
Invoke-Checked cargo @('fetch', '--locked')
Invoke-Checked cargo @('build', '--locked', '--release', '--target', $target, '-p', 'nerv')
$binary = "target/$target/release/nerv.exe"
Copy-Item -LiteralPath "$mingwRoot/bin/libunwind.dll" -Destination "target/$target/release/libunwind.dll" -Force
$prepared = Join-Path $root ('target/test-linker-' + [guid]::NewGuid().ToString('N'))
Invoke-Checked python @('tools/prepare-linker.py', '--target', $target, '--source', $mingwRoot, '--llvm', $env:LLVM_SYS_231_PREFIX, '--destination', $prepared, '--binary', $binary)
$env:NERV_TOOLCHAIN = $prepared
Invoke-Checked cargo @('test', '--locked', '--workspace', '--target', $target)
Invoke-Checked python @('-m', 'unittest', 'discover', '-s', 'tools', '-p', 'test_release*.py')
$payload = "target/payload/nerv-$version-windows-x64"
Invoke-Checked python @('tools/package-release.py', '--version', $version, '--target', $target, '--llvm', $env:LLVM_SYS_231_PREFIX, '--toolchain', $mingwRoot, '--payload', $payload, '--output', 'dist/release')
$iscc = & "$PSScriptRoot/setup-installer.ps1"
Invoke-Checked python @('tools/build-installer.py', '--payload', $payload, '--output', 'dist/release', '--version', $version, '--platform', 'windows', '--iscc', $iscc)
