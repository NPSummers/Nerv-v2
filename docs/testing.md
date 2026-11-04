# Compiler regression tests

From the repository root, run the entire Rust compiler and runtime suite with one command:

```sh
bash ./test.sh
```

On Windows:

```powershell
.\test.ps1
```

These commands require Rust 1.94 or newer and native build tools. They reuse the LLVM setup scripts described in the [README](../README.md), including the first-run LLVM download and macOS zstd requirement. Once LLVM is configured in your shell, the underlying command is `cargo test --workspace --locked`. Cargo test arguments can be forwarded, for example `bash ./test.sh --test regressions -p nerv`.

## Coverage

- Unit tests in the lexer, syntax, semantic checker, JIT, and runtime crates.
- Executable specification examples and module/language-rule checks in `crates/nerv-cli/tests/language_rules.rs`.
- CLI fixtures in `crates/nerv-cli/tests/fixtures`, run by `crates/nerv-cli/tests/regressions.rs`: parsing, checking, execution, and compile-time/runtime failures. They cover structs, enums, traits, methods, arrays, lambdas, multi-file imports, output, and exit status.
- Exact diagnostic snapshots in `crates/nerv-cli/tests/diagnostics`, run by `diagnostics.rs`: lexical/parser errors, type mismatches, unresolved names, invalid calls, imported-file errors, independent errors, EOF, and backend errors. Only the absolute fixture-root path and platform line endings are normalized.

For a diagnostic change, add a `.nerv` reproducer and its reviewed `.stderr` output under `tests/diagnostics`, then register its CLI mode in `diagnostics.rs`. Run `bash ./test.sh -p nerv --test diagnostics` (or `.\test.ps1 -p nerv --test diagnostics`). Do not update snapshots merely to silence a failure: check the selected file, line, column, underline, and explanatory notes.

## Adding a regression

For every compiler bug fix, add a minimal reproducer that fails before the fix and passes afterward. Use a unit test for an internal invariant; use a CLI fixture for user-visible behavior. Keep bug-specific tests even when broader examples cover similar features.

To add a CLI fixture:

1. Create a directory under `tests/fixtures/{parse,check,run,fail}` with `main.nerv`. Put imported modules alongside it, retaining their relative paths.
2. Add `expect.txt`: the CLI mode (`--parse`, `--check`, or `--run`) on the first line and the exact process exit code on the second.
3. Add `stdout.txt`, including its final newline, or an empty file for no output. Successful parser fixtures instead use a nonempty AST-output substring to avoid coupling every test to the whole debug representation.
4. For failures, add `stderr.txt` with a nonempty diagnostic substring. Avoid absolute paths and OS-specific error text. Successful fixtures require empty stderr.
5. Register the fixture with a descriptive test name in the `fixtures!` invocation in `regressions.rs`.

Tests run the built CLI in each fixture's directory and normalize CRLF to LF, but do not trim program output. A Nerv main result is printed on stdout; it is **not** the process exit code. The output fixture explicitly checks this distinction.

## CI and platform scope

`.github/workflows/compiler-tests.yml` runs the full workspace on Apple Silicon macOS and x64 Windows for pushes and pull requests. Linux builds and tests the LLVM-independent lexer, syntax, semantic checker, and runtime crates:

```sh
cargo test --locked -p nerv-lexer -p nerv-syntax -p nerv-sema -p nerv-runtime
```

Full Linux JIT/CLI CI is deferred until a supported LLVM 23.1 setup is available. Linux users with a compatible installation can set `LLVM_SYS_231_PREFIX` and the platform library search path, then run `bash ./test.sh`; it does not download LLVM on Linux. Intel macOS likewise requires an existing compatible LLVM installation.
