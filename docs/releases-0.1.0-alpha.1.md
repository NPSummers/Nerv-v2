# Nerv 0.1.0-alpha.1

Project tooling is now available through `nerv new`, `run`, `build`, `check`, `test`, and `fmt`.

- `Nerv.toml` describes applications, libraries, and local path dependencies.
- `Nerv.lock` records deterministic resolution and manifest checksums; `--locked` enforces it.
- Project applications forward program arguments and return integer main values modulo 256 as exit codes.
- LLVM object caching fingerprints compiler, libraries, loaded source contents, lock resolution, build options, and native CPU features. Every build still invokes the native linker.
- The formatter preserves comments and token structure and supports `--check`.
- Compiler archives bundle LLVM, runtime/standard library support, documentation, and notices. The native linker/build tools are still required to build applications.

Windows x64 was validated locally with 176 passing workspace tests and an extracted-package project smoke test. Apple Silicon packages are produced by the release workflow; inspect its status before treating that artifact as validated. Linux and Intel macOS have no prebuilt packages in this release.

Existing `nerv file.nerv --run` and `nerv build file.nerv` retain their historical printed-result behavior. Use project commands or `nerv run file.nerv -- args` for process semantics. Dependency imports currently share the language's global symbol namespace. Test isolation is per source file. Collections, generics, runtime ownership, and other alpha language limits are documented in the bundled guides.
