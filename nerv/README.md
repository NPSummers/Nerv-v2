# Nerv compiler written in Nerv

This compiler parses, checks, and compiles Nerv programs through LLVM's C API. It builds subsequent generations of itself without invoking the Rust compiler or another Nerv compiler. The native runtime and LLVM remain native dependencies.

The development bootstrap is isolated under `bootstrap/`. It is a copy of the Rust compiler with native-call and low-level runtime facilities added so it can build the first generation. `../Nerv-v2` remains unchanged. The published 1.0.0 executable cannot compile the new native-call intrinsics; use the isolated bootstrap for this source.

## Commands

```sh
nerv new hello
cd hello
nerv check --locked
nerv run --locked
nerv test --locked
nerv fmt --check
nerv build --release --locked -o hello.exe
```

A portable installation includes the compiler, standard sources, LLVM shared library, prebuilt runtime, and linker support. It does not need Rust, Visual Studio, or a second Nerv compiler to compile applications. Explicit output names work across platforms; `.exe` is only required for Windows applications.

`run -- arguments` forwards arguments and the application's exit status. Project checks scan `src` and `tests`; `--check FILE` checks that file and its import graph. `build --emit-object` emits a native object. `--no-cache` forces code generation. Builds serialize per output and publish completed files by rename. The object cache validates content hashes of the compiler, LLVM, source graph, configuration, and generated object; linking runs on every build.

Inspection modes remain available:

```sh
nerv --lex src/main.nerv
nerv --tokens src/main.nerv
nerv --parse src/main.nerv
nerv --ir src/main.nerv --locked
```

See [PROJECTS.md](PROJECTS.md), [SEMANTICS.md](SEMANTICS.md), [AST.md](AST.md), [TRAITS.md](TRAITS.md), and [NATIVE.md](NATIVE.md).

## Build and verify the bootstrap

On this Windows development machine, from this folder:

```powershell
python tools/bootstrap.py --offline `
  --llvm ../Nerv-v2/llvm `
  --toolchain ../Nerv-v2/dist/published-1.0.0/extracted/nerv-1.0.0-windows-x64/toolchain `
  --cc ../Nerv-v2/target/toolchains/llvm-mingw/bin/x86_64-w64-mingw32-clang.exe
```

Rust and LLVM are development requirements. The script builds the isolated bootstrap, builds three Nerv-written compiler generations, compares emitted objects, runs unit and differential/executable tests, and writes `target/bootstrap-proof.json`. A toolchain directory must contain `Nerv-toolchain.toml` with the native target and linker argument template. Linux/macOS require their native toolchain/runtime; a Windows runtime archive cannot be used for those hosts.

The resulting compiler is `target/stage3.exe` on Windows or `target/stage3` on Unix. `target/native.json` holds development linker settings. `NERV_LLVM`, `NERV_NATIVE_CONFIG`, and `NERV_STDLIB` override development asset locations. Portable installations discover their assets beside the executable, including invocation through PATH.

## Package a portable build

```powershell
python tools/package-portable.py --compiler target/stage3.exe --llvm-library ../Nerv-v2/llvm/bin/LLVM-C.dll
```

This stages `target/portable`, compiles and runs a standard-library program with development overrides removed, and creates a ZIP on Windows or tar.gz on Unix. The package uses relative asset paths so it can be moved. Native platform verification results belong in the bootstrap proof; cross-target object emission alone does not establish native execution compatibility.

## Current compatibility boundaries

Static traits are supported; dynamic trait objects, associated types, generic trait declarations, multiple trait bounds, and applied-generic impl syntax remain outside the implemented language subset. Claimed trait methods are validated more strictly than stage 0. Qualified enum patterns are a tracked extension. The manifest reader implements the project's string/table schema rather than general TOML. See the linked references for details.
