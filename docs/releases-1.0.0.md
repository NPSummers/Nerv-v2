# Nerv 1.0.0

Nerv 1.0.0 adds self-contained compiler distributions and native installers. Installed compilers include LLVM, their linker, and target link support. Ordinary local Nerv projects compile without separately installing Rust, Visual Studio, a Windows SDK, Xcode, GCC, or Clang. Generated applications run without Nerv or LLVM installed.

## Downloads

| Platform | Downloads | Minimum system |
| --- | --- | --- |
| Windows x64 | Per-user setup EXE and portable ZIP | Windows 10 with UCRT |
| macOS arm64 | Installer PKG and portable ZIP | macOS 15 |
| Linux x64 | Portable tar.gz | glibc 2.39 |

The release manifest records final artifact hashes, source commit, producer versions, and signing status. Verify downloads against SHA256SUMS and read the [installation instructions](installation.md).

Windows setup offers user PATH integration and an uninstaller. macOS installs versioned payloads and a managed public symlink; its bundled uninstall script preserves files outside the installed-file inventory. Linux downloads require extraction only.

## Language and library

The compiler supports content-based string equality, qualified module namespaces, capturing closures, nested early returns, break/continue, and Result/Option propagation with `?`. The standard library includes text formatting/parsing, structured I/O errors, paths, growing buffers, string-keyed hash maps, allocation scopes, and cooperative tasks/cancellation. JSON, HTTPS, and WebSocket support are native runtime features.

Project commands include `new`, `check`, `test`, `run`, `build`, and `fmt`. The compiler embeds its standard library and prebuilt runtime; compilation does not invoke Cargo on the user's machine.

## Compatibility policy

Starting with 1.0.0, documented language behavior, public standard-library interfaces, project/lockfile formats, and documented CLI commands/options follow Semantic Versioning. Backward-compatible fixes use PATCH releases; backward-compatible additions use MINOR releases; incompatible public changes require a MAJOR release. Reserved or explicitly unsupported language constructs are outside the supported contract.

Generated LLVM IR, internal runtime symbols, object-cache formats, and compiler diagnostics wording are implementation details. Mixing objects or embedded runtime archives from different compiler builds is unsupported; rebuild applications from source with the selected compiler. Generated applications retain their documented operating-system dependencies.

## Validation

The distribution implementation passed 206 workspace tests on each initial native target. Portable tests cover extraction, relocation into spaces/Unicode paths, symlink invocation on Unix, standalone execution, and explicit broken-toolchain failures. Installer tests cover payload identity, installation/reinstallation, uninstall, PATH integration on Windows, and preservation of project files. Linux portables were additionally checked offline on Ubuntu 24.04, Debian trixie, and Fedora 43 without development tools.

Version 1.0.0 artifacts are rebuilt and verified from the release commit before publication. Artifact signing status is stated explicitly in the release manifest and release notes.
