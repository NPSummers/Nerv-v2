# Nerv language roadmap

The goal is to make Nerv useful for real applications, not just examples and loop benchmarks. Complete each milestone with tests and working examples before expanding the language further.

## 1. Define the language rules

- [x] Write a short language specification describing syntax and behavior.
- [x] Define integer widths, signedness, conversions, and overflow behavior.
- [x] Define floating-point types and conversion rules.
- [x] Define variable scope, shadowing, mutability, and evaluation order.
- [x] Define function calls, returns, and expression-valued blocks.
- [x] Define module resolution, visibility, and import cycles.
- [x] Specify behavior for out-of-bounds indexing, division by zero, and failed assertions.
- [x] Choose a memory model: garbage collection, reference counting, or ownership.
- [x] Specify string encoding, array lifetimes, and how values are copied or shared.

Specified in [the alpha language specification](../docs/language-spec.md), with executable examples in `crates/nerv-cli/tests/language_rules.rs` and runtime ownership tests. The chosen model is execution-region ownership. Numeric widths, global import visibility, and other alpha limits are explicit; later milestone implementations and remaining limits are recorded below.

**Done when:** each rule has a small example and an automated test, including invalid programs where appropriate.

## 2. Establish a compiler regression suite

- [x] Add fixtures for parsing, type checking, execution, and expected failures.
- [x] Cover existing structs, enums, traits, methods, arrays, lambdas, and imports.
- [x] Assert program output and process exit status.
- [x] Add regression tests whenever a compiler bug is fixed (documented contribution policy).
- [x] Test the Rust compiler crates in CI.
- [x] Add macOS, Windows, and Linux build jobs as platform support becomes available.

Implemented in the [compiler regression suite](../docs/testing.md). Run `bash ./tools/test.sh` on macOS or `.\tools\test.ps1` on Windows. CI runs the full workspace on macOS and Windows; Linux currently covers LLVM-independent frontend and runtime crates. Full Linux JIT/CLI coverage remains deferred until supported LLVM setup is available.

**Done when:** one documented command runs the compiler tests, and CI catches changes to existing behavior.

## 3. Improve source locations and diagnostics

- [x] Represent source files with stable identifiers.
- [x] Attach source spans to expressions, statements, declarations, and types.
- [x] Preserve spans through later compiler stages.
- [x] Display the file, line, source excerpt, and underline for source-located diagnostics.
- [x] Add useful notes for type mismatches, unresolved names, and invalid calls.
- [x] Report multiple independent errors without flooding the user with follow-up errors.
- [x] Add snapshot tests for diagnostic output.

Implemented in the [source locations and diagnostics](../docs/diagnostics.md) infrastructure, with CLI snapshots in `crates/nerv-cli/tests/diagnostics`. Recovery reports one error per function, up to 20; syntax/loading/declaration failures stop early. Runtime and LLVM infrastructure errors without a known source operation remain plain messages. Explicit type occurrences retain spans separately from location-independent semantic types.

**Done when:** common errors point at the exact offending expression and explain what was expected.

## 4. Introduce typed intermediate representation

- [x] Add an internal representation between semantic checking and LLVM generation.
- [x] Resolve names to stable symbol identifiers.
- [x] Store checked types explicitly on expressions and bindings.
- [x] Resolve function calls, methods, fields, and enum constructors before code generation.
- [x] Represent conversions explicitly instead of rediscovering them in the backend.
- [x] Preserve source spans for diagnostics and future debug information.
- [x] Change the JIT to consume checked IR rather than syntax modules.
- [x] Keep existing execution behavior covered by regression tests during the migration.

Implemented in `nerv-sema::ir` and the [checked IR pipeline](../docs/typed-ir.md). The JIT accepts only checked programs, uses resolved IDs/indices/signatures, and emits explicit conversions. Semantic restrictions are diagnosed by `--check` before LLVM. Existing behavior remains covered by the full compiler suite, with added invariant and execution regressions. The frontend currently uses diagnostic checking followed by IR resolution; combining those passes is a future optimization, not backend work.

**Done when:** LLVM generation consumes resolved, typed operations and does not repeat semantic checking.

## 5. Implement the memory and runtime model

- [x] Implement the lifetime and allocation rules selected in step 1.
- [x] Define representations for strings, arrays, structs, and enum payloads.
- [x] Handle allocation failures and runtime errors consistently.
- [x] Implement bounds checks and the specified arithmetic checks.
- [x] Test nested values, function returns, shared values, and cleanup behavior.
- [x] Check for leaks and invalid memory access with appropriate tooling.
- [x] Define a runtime interface shared by JIT and standalone programs.

Implemented and validated in the [runtime ABI and ownership model](../docs/runtime-abi.md). Runtime tests pass under strict-provenance Miri; fatal-exit/allocation-failure checks run as subprocesses. Native macOS leak checking reports zero leaks for the nested/shared-value example. Experimental integer-handle collections remain outside the safe core and strict-provenance validation.

**Done when:** programs can create, pass, return, and release compound values safely under the documented rules.

## 6. Add standalone compilation

- [x] Separate LLVM module generation from JIT execution.
- [x] Emit object files for the host platform.
- [x] Link generated objects with the Nerv runtime and required system libraries.
- [x] Add `nerv build main.nerv -o app`.
- [x] Define debug and release build modes.
- [x] Preserve user-visible behavior between JIT and compiled execution.
- [ ] Test generated executables on machines without the compiler or LLVM installed (fresh-runner CI is configured; awaiting its first successful run).
- [x] Document supported targets and native linker requirements.

Implemented in [standalone compilation](../docs/standalone.md), with both-mode JIT/native parity tests and dependency inspection. Local ARM64 macOS tests run generated applications with empty PATH and LLVM-related environment removed; the verified sample links only to system `libSystem`. The clean-runner macOS/Windows CI transfer test must still pass before marking clean-machine validation complete. Windows/Linux native execution is not locally verified.

**Done when:** a Nerv program can be distributed and executed without a Nerv development environment.

## 7. Complete core types and structured errors

- [x] Finish the specified numeric types and explicit conversions across all compiler stages.
- [x] Add generics with clear rules for type inference and constraints.
- [x] Support generic structs, enums, and functions.
- [x] Provide optional values and result values through ordinary language types where possible.
- [x] Check pattern matches for exhaustiveness.
- [x] Define error propagation and unrecoverable failures.
- [x] Test interactions between generics, traits, methods, and enum payloads.

Implemented in the [language specification](../docs/language-spec.md#12-generics-and-recoverable-errors), with numeric, generic, compound-payload, and standalone regressions. Generics specialize concrete uses; inference rules and the 256-instance limit are documented. Errors propagate through explicit exhaustive matches. Generic methods and generic traits remain outside this milestone.

**Done when:** reusable collections and recoverable errors do not require compiler special cases for each value type.

## 8. Build a small standard library

- [x] String manipulation and UTF-8 operations.
- [x] Growable arrays and iteration.
- [x] Maps and sets.
- [x] File and directory operations.
- [x] Standard input, standard output, and standard error.
- [x] Command-line arguments and environment variables.
- [x] Time, durations, and random numbers.
- [x] Assertions and test utilities.
- [x] Structured I/O errors.
- [x] Documentation and runnable examples for public APIs.

Implemented in the [small standard library](../docs/standard-library.md), embedded as `nerv.core` and `nerv.std`. Includes generic value collections, string sets/maps, typed I/O errors, process inputs, and time utilities. Collections copy on update and use linear scans; these initial implementations target small applications. The file-processing CLI and JIT/debug/release I/O tests exercise the supported APIs.

**Done when:** a small command-line application can handle input, process data, and write results using supported library APIs.

## 9. Add project tooling

- [x] Introduce a project manifest and define the project layout.
- [x] Add `nerv new`, `nerv run`, `nerv build`, `nerv check`, and `nerv test`.
- [x] Preserve a simple way to run standalone source files.
- [x] Define program arguments and exit-code behavior.
- [x] Add deterministic dependency resolution and a lockfile.
- [x] Support local dependencies before adding remote packages or a registry.
- [x] Add build caching with correct invalidation.
- [x] Build a formatter and expose it through `nerv fmt`.
- [x] Publish installation instructions and compiler releases.

Implemented in [project tooling](../docs/projects.md) and [installation](../docs/installation.md): `Nerv.toml`, deterministic local `Nerv.lock`, project commands, object caching with fresh native linking, and a token-preserving formatter. Project applications use argument forwarding and process exit codes; historical file/JIT behavior remains available. The Windows x64 alpha compiler is packaged with LLVM and checksums; tag-based CI additionally packages Apple Silicon releases. Linux binary packaging and remote package registries remain deferred.

**Done when:** another developer can create, build, test, and share a Nerv project without editing repository scripts.

## 10. Add editor and debugging support

- [ ] Provide syntax highlighting.
- [ ] Add an LSP server that reuses compiler parsing and checking.
- [ ] Support diagnostics, hover information, completion, and go-to-definition.
- [ ] Add find-references and rename once symbol tracking is reliable.
- [ ] Emit source-level debug information for compiled programs.
- [ ] Document how to debug a Nerv executable with platform tools.

**Done when:** ordinary editing and debugging do not require repeatedly running the compiler by hand.

## 11. Validate with real applications

- [ ] Build a file-processing CLI.
- [ ] Build a small multi-module application with tests.
- [ ] Record missing features and confusing diagnostics encountered while building them.
- [ ] Prioritize demonstrated limitations over speculative syntax additions.
- [ ] Benchmark startup time, compilation time, memory use, and representative workloads.
- [ ] Keep benchmark workloads and optimization settings comparable across languages.
- [ ] Document known limitations and compatibility expectations before a public release.

**Done when:** someone other than the language author can build and maintain a useful Nerv program.

## Recommended first milestone

Start with language rules and regression coverage, then source spans and typed IR. Implement the runtime decisions needed for standalone compilation, and ship a small compiled CLI before investing in a package registry, async runtime, or advanced metaprogramming.
