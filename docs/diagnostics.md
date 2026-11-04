# Source locations and diagnostics

The compiler stores source text in a compilation-owned `SourceMap`. The module loader canonicalizes paths, assigns each file one `SourceId`, and reuses it for repeated imports. IDs are stable within that compilation, not persistent identifiers across compiler invocations. Diagnostics retain the original source text instead of rereading files when displayed.

`Span` is a source ID and a half-open UTF-8 byte range. Every expression and statement has a span; declarations, parameters, fields, enum variants, imports, and match arms do too. `Module.type_uses` retains source occurrences of explicit types, including nested array and function types. Semantic `Type` values remain location-independent so two occurrences of `i64` still compare as the same type. Implicit types have no source occurrence.

`parse_source` accepts an explicit source ID; the convenience `parse` API uses ID zero for a standalone buffer. AST cloning preserves all spans and type occurrences. Semantic checking carries spans into [checked IR](typed-ir.md), including lifted lambdas. The JIT consumes only that IR. Backend errors carry spans when a source operation is known; semantic restrictions are checked before code generation.

CLI frontend and located backend diagnostics show the path, one-based line and character column, original source line, and a caret underline. Tabs expand to four spaces in excerpts. Ranges spanning several lines underline their first line; empty ranges and EOF still have a visible caret. Notes explain common type mismatches, unknown names, and incorrect function/method arguments. Errors in imported functions are attributed to that imported file; bad arguments are attributed to the caller, not the callee's parameter declaration.

Semantic recovery uses **function boundaries**: after the first error in a function, checking continues with other functions. This avoids guessing types for failed bindings and emitting dependent errors. At most 20 errors are reported, with a note when the cap is reached. Declaration conflicts, module loading errors, and lexical/parser failures currently stop that compilation; recovery within a function or malformed syntax is not yet implemented.

Runtime failures and LLVM infrastructure errors that cannot be tied to a source node retain plain messages. Runtime instruction-level source tracking and multi-line/terminal-display-width rendering are future improvements. Rendering currently counts Unicode scalar values, not grapheme clusters or double-width terminal cells.

See [testing](testing.md) for the regression command and diagnostic snapshot conventions.
