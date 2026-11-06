# Nerv projects

Create an application with `nerv new hello`, or a library with `nerv new --lib math`. Package names and dependency aliases use ASCII identifiers. Projects contain `Nerv.toml`, `Nerv.lock`, `src/main.nerv` or `src/lib.nerv`, optional `tests/*.nerv`, and ignored `target/nerv/` output. Commands search upward from the current directory for `Nerv.toml`; `--manifest-path path/to/Nerv.toml` selects a project explicitly.

```sh
nerv new hello
cd hello
nerv check
nerv test
nerv fmt --check
nerv build --release
nerv run --release -- "argument with spaces" --flag
```

The repository launchers forward these commands too, including when invoked by absolute path from another directory. Installed compilers need no repository scripts, Rust, or Cargo. See [installation](installation.md).

## Manifest and local dependencies

```toml
[package]
name = "hello"
version = "0.1.0"
entry = "src/main.nerv"
lib = "src/lib.nerv"

[dependencies]
math = { path = "../math" }
```

`entry` and `lib` are optional and have the defaults shown. Source paths must remain within the package. Versions currently have three numeric components. Unknown keys and remote/version dependency specifications are rejected. A dependency is a local package with its own manifest and library entry. In `src/main.nerv`, `import math` loads that dependency's library; `import math.extra` loads `extra.nerv` next to its library entry. Sibling source imports continue to work, with a fallback to the package's `src/` directory. Imports inside a dependency resolve using that dependency's own manifest. A package with a library can import it by its package name, including from integration tests.

Dependency resolution follows the declared paths, detects cycles, and writes `Nerv.lock` in stable order. The lock records package names, exact versions, relative manifest paths, dependency edges, and SHA-256 manifest checksums. Commit the manifest and lock together. Source edits rebuild code without changing the lock. Manifest edits require re-resolution. `--locked` fails on a missing, edited, or stale lockfile; it never updates it. The current resolver has no registry, network fetching, version ranges, or automatic upgrades. Move related local packages together to preserve their relative paths. Dependencies on another filesystem volume are rejected because their lock paths cannot be portable.

Imported declarations still share the language's global namespace; dependency aliases select files and do not introduce symbol namespaces. Packages must avoid conflicting function/type names. Libraries are compiled with their importing application; `check` and `test` support library-only packages, while `build` and `run` require `main`.

## Commands and process behavior

| Command | Behavior |
| --- | --- |
| `nerv new [--lib] PATH` | Creates a new directory, manifest, lock, source, test block, and ignore file; refuses existing directories. |
| `nerv check [file.nerv] [--locked]` | Checks the selected file, or all project source and integration-test files. |
| `nerv test [file.nerv] [--locked]` | Runs test blocks in the project source files and recursively under `tests/`; each file runs in a child process so fatal failures do not suppress later files. |
| `nerv build [--release] [--cpu generic\|native] [-o path] [--locked] [--no-cache]` | Emits a standalone application under `target/nerv/debug/` or `target/nerv/release/`. |
| `nerv run [file.nerv] [--release] [--cpu generic\|native] [--locked] [--no-cache] [-- arguments...]` | Builds and runs a standalone program; arguments after `--` pass through unchanged. |
| `nerv fmt [file.nerv] [--check]` | Formats project `src/` and `tests/`, or a selected file. `--check` reports changes and exits 1 without writing. Dependencies are not formatted. |

Project applications and `nerv run` use process semantics: an integer main returns its value modulo 256 as the exit status; a void main exits 0. Neither prints an implicit result. `arguments()` includes the executable at index zero and the program arguments after it. The compiler's own options are excluded. Runtime failures exit 1. `nerv run` forwards the child exit status; signal termination maps to 1. Compiler/check/build errors exit 1. Build messages go to stderr so stdout can be piped as program data.

Standalone source files remain supported: `nerv run main.nerv -- args` or `nerv main.nerv` uses process semantics. Historical `nerv main.nerv --run` and `nerv build main.nerv -o app` preserve result-printing/exit-zero behavior for existing scripts and benchmarks. The repository launchers preserve that historical single-file invocation. Legacy `--lex`, `--parse`, `--check`, `--test`, and `--bench` remain available.

Each source file's test blocks execute once; imported test blocks execute only in their own file target. Test runs count passing/failing files; assertions inside each file use the current runtime's fatal-failure semantics. The subprocess isolates files, not individual test blocks within a file.

## Cache and formatter

The build cache stores native LLVM object files, with content fingerprints of the compiler, bundled/configured LLVM library, loaded source texts including imports and the standard library, resolved lock, build mode, CPU target, and native host CPU/features. File modification times do not substitute for source contents. Cache object hashes detect corruption. `--no-cache` forces fresh generation. Objects live alongside the executable as `.nerv-object` and `.nerv-cache`; builds sharing an output are serialized with a file lock; removing `target/` clears them. The native link step always runs, so linker configuration and system libraries are used on every build, including cache hits. Failed links do not replace a previous executable.

The formatter normalizes indentation to four spaces, token spacing, line endings, and trailing whitespace while retaining comments, strings, and line breaks within expressions. It parses every selected source before writing anything and verifies that formatted token structure and syntax remain unchanged. Invalid input is reported without being rewritten. Formatting is idempotent. Symlinked files/directories discovered during project scans are skipped.

## Releases

Compiler archives include the embedded standard library/runtime, LLVM shared library, documentation, and license notices. Tagged releases run the workspace tests and package smoke tests before publishing Windows x64 and Apple Silicon archives and SHA-256 checksums. Manual workflow runs create downloadable build artifacts without publishing a release. Native application linking still requires the platform's build tools. Linux packaging and cross-compilation are not provided by this milestone.
