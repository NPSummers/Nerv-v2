# Rust and LLVM Rewrite Plan

## Goal

Replace the current Zig, C, and MIR implementation with a Rust compiler that:

- tokenizes source with Logos;
- lowers typed Nerv programs to LLVM IR;
- executes programs through LLVM ORC LLJIT;
- provides the Nerv runtime in Rust;
- preserves the language syntax, command-line behavior, diagnostics, and `.nerv` module format.

The completed implementation must contain no Zig compiler, C runtime, MIR wrapper, or vendored MIR source.

## Pinned Toolchain

Pin versions in the workspace instead of tracking LLVM or crate releases loosely.

| Component | Version | Use |
|---|---:|---|
| Rust | current stable, with an explicit MSRV in the workspace | compiler and runtime |
| Logos | `=0.16.1` | raw token recognition |
| LLVM | `23.1.1` | IR, optimization, target lowering, and JIT |
| llvm-sys | `=231.0.0` | direct Rust bindings to LLVM's C API |

Use `llvm-sys` directly for the LLVM integration. Inkwell 0.9 supports LLVM through 22.1, not LLVM 23, so it cannot be the primary binding for this migration. LLVM's ORC C API is marked experimental; pinning the LLVM and `llvm-sys` versions together is mandatory.

References:

- [LLVM 23.1.1 release](https://llvm.org/)
- [ORC JIT design](https://llvm.org/docs/ORCv2.html)
- [LLVM ORC C API](https://llvm.org/doxygen/Orc_8h.html)
- [LLJIT C API](https://llvm.org/docs/doxygen/group__LLVMCExecutionEngineLLJIT.html)
- [Logos](https://crates.io/crates/logos)
- [llvm-sys](https://crates.io/crates/llvm-sys)

## Target Layout

```text
Cargo.toml
crates/
  nerv-cli/        command-line parsing, file loading, diagnostics, commands
  nerv-lexer/      Logos raw tokens and layout token stream
  nerv-syntax/     AST, parser, spans, module imports
  nerv-sema/       name resolution, types, trait checks, diagnostics
  nerv-codegen/    typed AST to LLVM IR and optimization pipeline
  nerv-jit/        LLJIT ownership, module loading, symbol lookup
  nerv-runtime/    Rust implementations exported with the Nerv runtime ABI
  nerv-test/       compiler integration and differential test support
tests/
  cases/           source fixtures and expected output or diagnostics
  snapshots/       checked lexer, parser, type, and runtime results
```

Dependencies flow in one direction:

```text
cli -> syntax -> lexer
cli -> sema -> syntax
cli -> codegen -> sema
cli -> jit -> codegen
jit -> runtime
test -> cli
```

`nerv-runtime` must not depend on syntax, semantic analysis, code generation, or JIT state.

## Compatibility Contract

Before moving implementation code, record observable behavior of the current compiler:

- accepted and rejected syntax;
- token locations and indentation behavior;
- parser output shape for representative programs;
- type-check results and diagnostic text where practical;
- import resolution and circular-import failures;
- standard-library namespace behavior;
- `run`, `test`, `bench`, `--lex`, `--parse`, and `--check` exit codes;
- output from every example and regression case.

Add fixtures before changing a feature. Each fixture must state whether compilation succeeds, its stdout/stderr, and expected exit status. The Rust implementation replaces the old compiler only after it passes the same fixture suite.

## Lexer: Logos Plus Layout

Logos should produce raw lexical units only. A `LayoutLexer` wrapper owns line and column tracking and converts raw whitespace into the compiler token stream.

1. Define one Rust token enum for keywords, identifiers, literals, operators, delimiters, comments, whitespace, and invalid input.
2. Use Logos callbacks for number parsing, escape validation, and source spans.
3. Keep comments and horizontal whitespace out of the parser token stream.
4. On each meaningful newline, compare indentation with an integer stack and emit `Newline`, `Indent`, or `Dedent` tokens.
5. Suppress layout tokens inside parentheses, brackets, and braces.
6. Treat string interpolation as an explicit lexer mode or a dedicated string scanner; do not make parser rules infer interpolation boundaries.
7. End every file with the required dedents followed by `Eof`.

The lexer API should return `Token { kind, span }`. Spans use byte offsets with a source-file handle; line and column conversion belongs in diagnostics.

## Syntax and Semantic Analysis

Port the current AST as Rust enums and structs without redesigning the language during the migration.

- Allocate AST nodes in ordinary owned containers first. Introduce arena allocation only if profiling shows it is needed.
- Parse with a direct recursive-descent or Pratt parser matching the current precedence and layout rules.
- Keep declarations, expressions, patterns, and types separate in the AST.
- Resolve imports before type checking a module graph.
- Use explicit semantic IDs for symbols, functions, types, traits, and implementations rather than string keys after name resolution.
- Preserve current standard-library names and type behavior before adding generic or inference improvements.
- Keep diagnostics structured as `Diagnostic { severity, span, message, notes }` and render them only in `nerv-cli`.

Do not add a new IR before LLVM. The typed AST is the code-generation input for the first Rust implementation.

## LLVM Code Generation

`nerv-codegen` owns every LLVM handle and all `unsafe` calls to `llvm-sys`.

1. Initialize native target, asm printer, asm parser, and target information once.
2. Create an LLVM context, module, builder, target machine, host target triple, and data layout for each compilation unit.
3. Define a single Nerv ABI before lowering functions:
   - integer and boolean representation;
   - floating-point representation;
   - pointer and string representation;
   - array, tuple, option, and result layout;
   - calling convention, linkage, alignment, and ownership rules.
4. Lower declarations before bodies so recursive and cross-module calls have stable LLVM function references.
5. Lower expressions and statements into basic blocks with explicit terminators.
6. Emit checked control-flow joins for `if`, loops, `match`, option, and result operations.
7. Declare every runtime entry point from a single Rust ABI table shared by code generation and `nerv-runtime`.
8. Verify each generated module with LLVM verification before it reaches the JIT.
9. Run a named, reproducible optimization pipeline selected by `--opt=0..3`.

Keep IR verification enabled in tests and debug builds. On failure, write LLVM IR beside the failing test artifact rather than continuing to execute it.

## JIT Design

Use ORC LLJIT, not the legacy execution engine.

```text
typed module
    -> LLVM IR module
    -> LLVMOrcThreadSafeModule
    -> LLJIT main JITDylib
    -> optimized native code
    -> lookup exported Nerv entry point
    -> extern "C" call boundary
```

`nerv-jit` is responsible for:

- creating and disposing LLJIT and thread-safe contexts;
- registering process symbols and Nerv runtime symbols;
- adding an LLVM module to the correct JITDylib;
- looking up the generated entry function;
- retaining modules and runtime state until execution completes;
- converting LLVM errors into Rust diagnostics.

All JIT function-pointer casts live in one small audited module. Calls across the JIT boundary use `extern "C"` signatures only. The code generator and runtime must share ABI tests for every exported function.

## Rust Runtime

Port `runtime/nervrt.c` and `src/nerv_rt.c` into `nerv-runtime` in stages.

1. Define runtime exports as `pub extern "C"` functions with stable, documented ABI names.
2. Register their addresses in the ORC main JITDylib before executing generated code.
3. Port pure functions first: printing, math, strings, conversions, arrays, tuples, option/result, and formatting.
4. Port stateful handles next: collections, files, clocks, networking, synchronization, and threading.
5. Replace integer-cast handles with opaque Rust allocation types where the existing public language ABI permits it. Keep raw handles only behind the runtime boundary.
6. Add lifecycle tests for every allocation and handle API before deleting its C equivalent.

No generated LLVM code may depend on Rust's internal ABI. All runtime calls use the declared C ABI.

## Build and Developer Workflow

Use a Cargo workspace at the repository root.

- `build.rs` locates the pinned LLVM installation through `LLVM_CONFIG_PATH` or an explicit `NERV_LLVM_PREFIX`.
- Fail early if `llvm-config --version` does not match the supported 23.1 release line.
- Keep LLVM discovery and linker flags in one crate, not scattered through build scripts.
- Provide `cargo test`, `cargo run -- <file.nerv>`, `cargo bench`, and a CI matrix for Windows, Linux, and macOS.
- Cache the LLVM installation in CI; do not build LLVM from source for ordinary test jobs.

## Migration Phases

### Phase 0: Freeze Behavior

- Add fixture coverage for examples, standard library calls, imports, diagnostics, test blocks, and benchmark blocks.
- Add a harness that runs the existing compiler and records stdout, stderr, and status.
- Define the public runtime ABI in one checked document and test suite.

Exit condition: the current compiler has a reliable baseline suite.

### Phase 1: Rust Workspace and CLI

- Add the workspace and `nerv-cli`.
- Implement command parsing, source loading, import path resolution, and diagnostics without compiling programs yet.
- Match existing CLI flags and exit behavior.

Exit condition: Rust CLI accepts the same commands and resolves the same files.

### Phase 2: Logos Lexer

- Implement raw tokens and `LayoutLexer`.
- Snapshot tokens and spans against Phase 0 fixtures.
- Add property and fuzz tests for indentation, malformed strings, and interpolation.

Exit condition: token kind and source position compatibility on the fixture corpus.

### Phase 3: Parser and AST

- Port AST and parser.
- Add parse snapshots and failure diagnostics.
- Preserve grammar and precedence; defer syntax changes.

Exit condition: every valid fixture produces the expected AST class and every invalid fixture fails at the expected source region.

### Phase 4: Semantic Analysis

- Port scopes, declarations, types, traits, pattern checks, and imports.
- Add typed-program snapshots or structured assertions.

Exit condition: `--check` is compatible with the baseline suite.

### Phase 5: Minimal LLVM JIT

- Add `llvm-sys`, LLVM initialization, module verification, LLJIT, and symbol lookup.
- Lower literals, arithmetic, local variables, direct functions, and `main`.
- Register a minimal Rust runtime for output.

Exit condition: simple Nerv programs execute through ORC LLJIT on all supported platforms.

### Phase 6: Language and Runtime Parity

- Add control flow, arrays, tuples, strings, closures, structs, enums, traits, match, option/result, imports, and standard-library dispatch.
- Port runtime groups with ABI and behavior tests.

Exit condition: all Phase 0 fixtures run on Rust and LLVM with matching observable behavior.

### Phase 7: Performance and Operations

- Define optimization levels and benchmark representative programs.
- Add IR verification, sanitizer jobs for runtime code, leak checks, and JIT lifecycle tests.
- Test concurrent compilation only after single-threaded correctness is stable.

Exit condition: performance is measured against the MIR implementation and regressions have thresholds.

### Phase 8: Cutover

- Make the Rust compiler the default `nerv` executable.
- Remove Zig build files, C runtime sources, MIR wrappers, and the vendored MIR directory.
- Keep the fixture suite and differential harness results as regression tests, not as a runtime fallback.

Exit condition: no production code path invokes Zig, C, or MIR.

## Risks and Controls

| Risk | Control |
|---|---|
| LLVM binding/API churn | pin LLVM 23.1.1 and `llvm-sys` 231.0.0; upgrade intentionally in a dedicated change |
| JIT ABI mismatch | single runtime ABI table, `extern "C"` only, ABI integration tests |
| Lexer layout regressions | token snapshots, indentation fuzzing, nested-delimiter tests |
| Behavior drift during port | dual-compiler fixture harness until cutover |
| Windows symbol resolution | CI coverage from the first LLJIT milestone, not at the end |
| Runtime memory bugs | Rust-owned runtime state, opaque handles, Miri/unit tests where applicable, platform sanitizers |
| Unbounded scope | no language redesign or new features until parity is complete |

## Definition of Done

- `cargo test` validates lexer, parser, semantic analysis, ABI, JIT, runtime, and integration fixtures.
- `cargo run -p nerv-cli -- run examples/hello.nerv` executes through LLVM ORC LLJIT.
- All supported commands operate from the Rust executable.
- LLVM IR verification is enabled in tests and debug builds.
- Runtime calls are Rust `extern "C"` exports registered with ORC.
- The repository no longer includes Zig, C, MIR, or build paths that depend on them.
