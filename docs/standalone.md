# Standalone programs

Build a native application from the repository root:

```sh
./nerv.sh build example/hello.nerv -o hello --release
./hello
```

On Windows:

```powershell
.\nerv.ps1 build example\hello.nerv -o hello.exe --release
.\hello.exe
```

An installed compiler exposes the same CLI directly:

```text
nerv build main.nerv [-o app] [--debug|--release] [--cpu generic|native] [--emit-object]
```

The default output is the source file stem in the current working directory (`main` or `main.exe`). Output parent directories must already exist. Source files, including imported files, are protected against accidental overwrite. Generated files are staged beside the destination, then renamed only after success; failed links preserve an existing output and remove their temporary files.

## Modes and object files

- `--debug` (default) uses LLVM O0 and unoptimized machine-code emission. Locals have one stack slot per binding/invocation, including inside loops. Full source-level DWARF/PDB debugging is **not yet implemented**; that belongs to the editor/debugging milestone.
- `--release` uses LLVM O3 and aggressive machine-code optimization. Both modes retain all language runtime checks and have the same observable behavior.
- `--emit-object` emits a host `.o`/`.obj` instead of linking an executable. Use `-o` to choose its path. This needs LLVM but not the native linker. The object contains a C `main` adapter and references the version-2 Nerv runtime ABI; it is not a self-contained executable.

Standalone code defaults to `--cpu generic` for portability within the host target. `--cpu native` uses the same host CPU/features as the JIT, but the resulting application may not run on older CPUs. Both use PIC relocations. There is no cross-compilation flag yet. Objects and the statically embedded runtime must target the same OS/architecture.

## Runtime and distribution

See [integer ranges and loop optimization](optimization.md) for proven bounds-check elimination, reused-divisor arithmetic, and vectorization limits.

The CLI build script compiles the dependency-free Rust runtime into a native static archive and embeds it in the compiler. It also records the native link libraries reported by that Rust toolchain. A later `nerv build` extracts that archive temporarily and links it with the generated object. No Cargo, Rust compiler, source checkout, runtime archive installation, or LLVM is needed **to run the resulting application**. Native system libraries and a compatible OS/architecture are still required.

The shared [runtime ABI](runtime-abi.md) enters an execution region, runs the entry function, prints its result (zero for void), and cleans up. An integer main result is output, not a process exit status: `fn main() -> i64 => 7` prints `7` and exits 0. Runtime failures match JIT stderr and exit 1.

## Host build prerequisites

| Host | Requirements for `nerv build` |
| --- | --- |
| Apple Silicon macOS | Compatible LLVM 23.1, Xcode Command Line Tools (`cc`), and zstd for first-time official LLVM setup. Locally verified on ARM64. |
| Intel macOS | An existing compatible Intel LLVM 23.1 installation and Xcode Command Line Tools; automatic LLVM download is not provided. |
| x64/ARM64 Windows MSVC | Compatible LLVM 23.1 and Visual Studio C++ Build Tools + Windows SDK. The linker is discovered with its SDK environment. The runtime uses a static CRT. CI covers x64; ARM64 native compilation is not locally verified. |
| Linux | An existing compatible LLVM 23.1 installation, a C linker driver (`cc`), and native libc/system development libraries. No automatic LLVM setup/full native CI is provided yet; Linux AOT is implementation-supported but not validated locally. |

The compiler itself is built with Rust 1.94 or newer. `NERV_LINKER` overrides the linker **executable path** (MSVC `link.exe` on Windows; a C linker driver on Unix). Unix also honors `CC` when `NERV_LINKER` is unset. These variables accept one executable path, not a shell command with flags.

To use the underlying CLI without the launchers, configure LLVM in the shell, then use `cargo run -p nerv -- build ...`. The compiler still needs LLVM at compile time; only the generated executable is independent of it.

## Validation

```sh
bash ./tools/test.sh -p nerv --test standalone
```

The suite compares JIT and native stdout, stderr, and exit status in both build modes for fixtures and executable specification examples. It covers compound returns, sharing, imports, methods, lambdas, conversions, arithmetic checks, allocation failures, Unicode paths, real object emission, safe output replacement, and missing-linker diagnostics. Applications run outside the repository with an empty PATH and LLVM/Cargo library-path variables removed. Dependency inspection rejects LLVM dynamic libraries.

CI also builds a release application, transfers **only that executable** to a fresh runner with no repository checkout, Nerv compiler, or downloaded LLVM installation, and executes it there. Those jobs must pass before clean-machine support is considered validated for a platform. Local macOS validation additionally ran a compiled nested/shared-value example under `leaks --atExit` with zero reported leaks. This does not constitute validation on every OS/architecture or a proof of memory safety.
