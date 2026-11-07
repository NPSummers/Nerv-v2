# Install Nerv

The distribution pipeline builds Windows x64, Apple Silicon macOS, and Linux x64 downloads. Unsigned development artifacts have passed native validation on all three platforms; no new public release has been published. Obtain artifacts from [compiler releases](https://github.com/NPSummers/Nerv-v2/releases) or a successful manual Actions run.

The layouts below describe the new pipeline. Older alpha archives may still require native linking tools; read the instructions for that specific release. Existing release tags must not be reused for these new artifacts.

A complete download includes Nerv, LLVM, a private linker, and the target link libraries. The runtime and standard library are embedded in Nerv. Keep the complete directory together. Compiling an ordinary local project needs no separate Rust, Visual Studio, Windows SDK, Xcode, GCC, Clang, or LLVM installation. Generated applications need only the documented operating-system libraries.

## Windows x64

Use `nerv-VERSION-windows-x64-setup-unsigned.exe` for a per-user installation. It defaults to `%LOCALAPPDATA%\Programs\Nerv`, offers optional user PATH integration, and registers an uninstaller. Open a new terminal after installation. Windows installers are unsigned; Windows signing is not configured yet.

For a portable installation, extract `nerv-VERSION-windows-x64-portable.zip`. Run `nerv.exe` directly or add its directory to PATH yourself. Extraction changes no system settings. Windows 10 with UCRT is the proposed baseline; a clean-machine validation remains required before claiming support.

The installer supports `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART`, `/DIR="C:\path\Nerv"`, and `/MERGETASKS=addpath`. Its `unins000.exe` accepts the silent switches too. Reinstalling updates installer-owned files and keeps unrelated project files. Uninstall removes the PATH entry only if the installer added it. Native portable, payload identity, PATH, reinstall, uninstall, and user-file preservation checks passed in [Windows validation](https://github.com/NPSummers/Nerv-v2/actions/runs/42328213420).

## Apple Silicon macOS

Use `nerv-VERSION-macos-arm64-installer-unsigned.pkg`, or extract `nerv-VERSION-macos-arm64-portable.zip` and run its `nerv` executable. The initial minimum is macOS 15: the LLVM shared library declares macOS 15.0 in its Mach-O load commands. Packaging rejects components declaring a newer minimum. Native portable, symlink, PKG installation/reinstallation, and uninstall checks passed in [macOS validation](https://github.com/NPSummers/Nerv-v2/actions/runs/42324316910).

The package installs under `/Library/Nerv/VERSION` and manages `/usr/local/bin/nerv`. Versions have separate payload directories and receipts. Installation refuses to replace an unrelated command. To remove a specific installed version:

```sh
sudo sh /Library/Nerv/VERSION/uninstall-macos.sh VERSION
```

The removal script deletes listed installer-owned files, preserves additional files, and removes the public symlink only if it points to that version. Other installed versions remain available at their full paths. It does not automatically select an older version.

Public macOS releases are unsigned by default, including v1.0.0, and their installer filenames explicitly include `unsigned`. They are not Apple-notarized. Normal macOS checks for downloaded software still apply. When optional Developer ID signing/notarization is enabled, the signed PKG is stapled and ZIP notarization is checked online. Gatekeeper and offline first-run behavior still need clean-Mac testing; no instruction to disable Gatekeeper is part of installation.

## Linux x64

Extract `nerv-VERSION-linux-x64-portable.tar.gz` and run its `nerv`, or add the extracted directory to PATH. There is no Linux installer, `.deb`, or `.rpm`.

The initial pipeline targets glibc 2.39 or newer, such as Ubuntu 24.04. It bundles non-system compiler dependencies and private link support. Generated applications use the system ELF loader and glibc. This is not an Alpine/musl distribution. The declared baseline and the Ubuntu, Debian, and Fedora container checks must pass before publication; older glibc versions are not claimed.

## Verify downloads

Compare SHA-256 values against `SHA256SUMS` from the same release:

```powershell
Get-FileHash .\nerv-VERSION-windows-x64-portable.zip -Algorithm SHA256
```

```sh
shasum -a 256 nerv-VERSION-macos-arm64-portable.zip
sha256sum -c SHA256SUMS --ignore-missing
```

`release-manifest.json` records source commit, target, toolchain, baseline, signing status, sizes, and hashes. Manual Actions artifacts have one `manifest-PLATFORM.json` per target. Checksums are generated after signing/notarization.

## First project

```sh
nerv new hello
cd hello
nerv check --locked
nerv test --locked
nerv run
nerv build --release --locked
```

Distribute `target/nerv/release/hello` (`hello.exe` on Windows). See [project tooling](projects.md) for local dependencies, lockfiles, program arguments, formatting, and caching.

## Build the compiler and distributions

Developer builds need Rust 1.94 or newer, Git, LLVM 23.1, and native producer tools. Those requirements do not apply to end users. Existing MSVC development builds still work. For Windows distribution builds, run `tools/build-distribution.ps1`; it provisions verified LLVM-MinGW and Inno Setup, builds the GNU/LLVM compiler, prepares its linker manifest, runs tests, and builds the ZIP and setup EXE. Run it from a fresh checkout/output directory. Inno Setup's license terms apply to the installer producer.

On macOS, install the producer's zstd dependency and run `bash tools/build-distribution.sh`. Developer builds use Xcode tools to prepare LLVM and installers. The installed payload uses a bundled linker, pinned Zig Darwin stubs, and a minimal POSIX iconv stub whose API provenance and BSD notice are included in the sysroot. On Ubuntu 24.04, install `patchelf`, GCC/native development libraries, and LLVM build dependencies listed in the workflow, then run the same script. Linux native build/package verification and offline Ubuntu 24.04, Debian trixie, and Fedora 43 container checks passed in [Actions validation](https://github.com/NPSummers/Nerv-v2/actions/runs/42322742132).

Payloads go to `target/payload/nerv-VERSION-PLATFORM`; final artifacts go to `dist/release`. `tools/verify-distribution.py ARCHIVE` extracts and relocates a portable, tests compilation with PATH empty, runs a generated executable separately, and verifies clear failures for broken/missing toolchains. Do not merge stale files from older releases into these directories.

## Release policy

The authoritative version is `crates/nerv-cli/Cargo.toml` package `nerv`, with a matching Cargo.lock entry. `python tools/release_version.py --tag vVERSION` checks both against strict SemVer 2.0.0. Only pushing that matching tag can publish; PRs and manual workflow runs create validation artifacts only. All three target builds, installer tests, final portable tests, and Linux distribution checks must succeed.

Installer numeric fields map `MAJOR.MINOR.PATCH` to `MAJOR.MINOR.PATCH.0`, with each number limited to 65535. These fields do not order updates: full SemVer remains in display identity, Windows uninstall metadata, macOS versioned paths/unique receipts, VERSION, and filenames. Explicit installation replaces the selected Windows payload regardless of prerelease suffix; macOS activates the explicitly selected version's symlink. There is no automatic updater or numeric-version comparison that merges alpha, beta, and final versions.

`alpha.10` follows `alpha.2` under SemVer. Build metadata has no precedence. Prereleases never become the stable latest release; an older stable tag does not displace a newer stable release. Published assets are immutable. Unpublished draft retries compare existing hashes, upload only missing assets, and refuse changed bytes. Fix a published problem through a new version/tag.

Configure the `release` GitHub environment for publication review. To enable optional macOS signing, set the repository variable `NERV_MACOS_SIGNING_ENABLED=true` and configure repository secrets `MACOS_CERTIFICATE_P12` (base64 P12 containing both signing identities), `MACOS_CERTIFICATE_PASSWORD`, `MACOS_APPLICATION_IDENTITY`, `MACOS_INSTALLER_IDENTITY`, `APPLE_ID`, `APPLE_APP_PASSWORD`, and `APPLE_TEAM_ID`. They are used only by trusted tag builds. No signing credentials are used by local or PR validation. When signing is enabled, missing signing inputs fail the tagged macOS job and block the whole release. With signing disabled, explicitly labeled unsigned macOS assets are accepted for publication.

Before the first public tag, run manual Actions validation, complete native macOS/Linux checks, audit third-party license/source obligations, and test on Windows/macOS machines without developer tools. GitHub hosted Windows/macOS runners contain developer tools; an empty PATH test alone does not replace those clean-machine checks. Publication and certificate purchases are separate from implementing these scripts.
