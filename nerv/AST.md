# Bootstrap syntax arena

`Ast.nodes` contains `Node` values addressed by integer IDs. `Ast.root` selects the module. Each node has a typed `NodeTag`, source `text`, an exclusive UTF-8 byte span, and ordered child IDs. One AST describes one source file; a future module loader must attach a source-file ID externally or extend the span representation.

Nodes are appended after their children. Parenthesis handling and qualified struct construction can leave earlier provisional expression nodes unreferenced. Consumers must traverse from the root, rather than treating every arena entry as a live declaration or expression. Error results expose no AST.

## Declarations and types

| Tag | Text | Children |
| --- | --- | --- |
| Module | Empty | Declarations in source order. |
| ModuleName / Import | Dotted path | None. |
| Function | Function name | Generics, parameters, result type, then expression or block body. Trait declarations omit the body. |
| Function for test/bench | `Test` or `Bench` followed by the raw quoted label | Body block only. |
| Generics | Empty | Generic parameters. |
| Generic | Parameter name | Optional constraint type. |
| Parameter / Field | Name | Type. |
| Struct | Name | Generics, optional implemented trait type, fields/methods in source order. |
| Enum | Name | Generics, variants. |
| Variant | Name | Payload types. |
| Trait | Name | Method declarations. |
| Impl | First path in the declaration | Optional `for` path as a type node, then methods. |
| TypeName | Type spelling | None. |
| ArrayType | Fixed-length spelling, or empty for `[]T` | Element type. |
| FunctionType | Empty | Parameter types, then result type. |
| AppliedType | Dotted type name | Type arguments. |

An omitted function result is represented by a synthetic `void` type node. Unlike explicit type occurrences, its span covers part of the surrounding function signature. Missing generic lists have zero-width spans. AST syntax does not rename methods or specialize generics.

## Statements and expressions

Blocks hold statements. `Let` text is the name, prefixed with `mut ` when mutable; children are the optional annotation and initializer. `Assign` holds target then value. `Return` has zero or one child. `While` holds condition/body. `For` text combines the variable name and `..` or `..=`, and holds lower bound, upper bound, and body. `ExprStatement` wraps an expression.

Literal nodes preserve source spelling, with the negative i64 minimum stored as a single integer node. Unary/binary text uses lexer operator names; children preserve operand order. Calls hold callee then arguments. Members/methods store their name and base followed by arguments where applicable. Index holds base/index. Construct stores the dotted type name and initializer nodes; each initializer stores a field name and value child. Arrays contain elements; Repeat holds value/length. Try wraps its operand.

If holds condition, then block, and optional else block. Match holds scrutinee then arms. Arm text is the variant path; its children are an optional binding-name node followed by its expression body. That binding-name node is not an evaluated expression. Lambda holds typed parameters, result type, and expression body.

## Parser state

Parser functions share a cursor array, error-message array, and a one-element array holding the arena buffer. This makes mutations visible across recursive calls under Nerv's current value/handle semantics. Cursor slots hold token position, first-error token position (`-1` before failure), and recursive parser depth. Only the first error is retained. Each loop stops after failure, and recursive type/expression/block entry checks the nesting limit.

Semantic analysis must decode literals, resolve imports/names, distinguish types by symbol identity, validate control-flow context, and reject type errors. Syntax acceptance alone does none of those tasks.

## Loaded modules

The parser still creates one source-local arena at a time. The loader combines these arenas by offsetting child IDs and records every combined node's source-module ID in `SourceGraph.node_sources`. Spans remain relative to the original source text. A synthetic module root contains declarations from all loaded files; its zero-width span is not a source extent. Semantic analysis uses this source table to preserve namespaces and emit source-tagged checked IR.
