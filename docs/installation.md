# Install Nerv

Nerv is alpha software. Download the archive for your platform and its matching `.zip.sha256` file from [compiler releases](https://github.com/NPSummers/Nerv-v2/releases). Keep the executable beside its bundled LLVM library; the compiler embeds its runtime and standard library. Generated applications run without Nerv or LLVM installed.

## Windows x64

Install Visual Studio C++ Build Tools with the Windows SDK to link native applications. Extract the release ZIP into a new directory, verify the SHA-256 checksum with `Get-FileHash`, and add the directory containing `nerv.exe` to your user PATH. The repository includes an optional installer that verifies the adjacent checksum file and refuses to replace an existing installation:

```powershell
.\tools\install.ps1 -Archive .\nerv-0.1.0-alpha.1-x86_64-pc-windows-msvc.zip
```

Its default destination is `%LOCALAPPDATA%\Nerv\bin`. Add that directory to PATH, open a new terminal, and run `nerv --version`. Use `-Destination` to select another directory. The installer does not edit PATH or require an administrator account.

## Apple Silicon macOS

Install Xcode Command Line Tools with `xcode-select --install`. Verify with `shasum -a 256 ARCHIVE.zip` against the adjacent checksum file, extract into a new directory, and add that directory to PATH. The optional repository installer verifies the checksum:

```sh
bash tools/install.sh ./nerv-0.1.0-alpha.1-aarch64-apple-darwin.zip
export PATH="$HOME/.local/share/nerv:$PATH"
nerv --version
```

These alpha binaries are locally signed for code integrity, not Apple-notarized. Follow your platform's normal process for opening trusted downloaded developer tools. Other host targets can build from source with a compatible LLVM 23.1 installation; there are no Linux or Intel macOS binary packages yet.

## First project

```sh
nerv new hello
cd hello
nerv check --locked
nerv test --locked
nerv run
nerv build --release --locked
```

Distribute `target/nerv/release/hello` (`hello.exe` on Windows). Add dependencies as local paths in `Nerv.toml` and commit `Nerv.lock`. See [project tooling](projects.md) for commands, cache behavior, arguments, exit codes, and formatting.

## Build the compiler from source

Install Rust 1.94 or newer, the native build tools above, and Git. Clone the repository. On Windows, dot-source `tools/setup-llvm.ps1`, then run `cargo build --locked --release -p nerv`. On macOS, install zstd with Homebrew, source `tools/setup-llvm.sh`, run `setup_llvm`, then run the same Cargo command. Add `target/release` to PATH and keep LLVM available to the compiler. The repository launchers set up LLVM and invoke Cargo for local development.

To produce a redistributable bundle from a clean checkout, run the full tests, build the release compiler, and invoke `python tools/package-release.py --version VERSION --target TARGET`. On Windows, build with `RUSTFLAGS=-C target-feature=+crt-static`. The packager verifies the manifest version, bundles notices, and runs the packaged compiler through `--version`, `new`, `check`, and `run` outside the repository. Tag `vVERSION` to run `.github/workflows/release.yml`; its package jobs must both succeed before the publication job uploads archives. Releases remain prereleases while Nerv is alpha.
