# Nerv alpha language specification

This document specifies the current supported core, not a promise that every reserved keyword is implemented. Examples marked `run`, `reject`, or `fail` are executable conformance tests in `crates/nerv-cli/tests/language_rules.rs`. `run` is the exact output, `reject` is a compile-time diagnostic substring, and `fail` is a runtime diagnostic substring with exit status 1.

Run the suite with LLVM configured in the current shell. On macOS:

```sh
bash ./tools/test.sh -p nerv --test language_rules
```

On Windows:

```powershell
.\tools\test.ps1 -p nerv --test language_rules
```

Omit `-p nerv --test language_rules` to include all compiler and runtime tests and the overall test score. The module examples are covered by the suite's multi-file test. Runtime ownership and cleanup also have unit tests in `nerv-runtime`.

## 1. Source, layout, and declarations

Source files are UTF-8 and use the `.nerv` extension. Identifiers are case-sensitive ASCII letters, digits, and underscores, and cannot begin with a digit. `--` begins a line comment. Newlines separate statements; indentation forms blocks. Each indentation increase opens a block; dedentation must return to an existing indentation level. A tab counts as four spaces; four spaces per level is recommended. Blank lines and comment-only lines do not open blocks. Newlines within parentheses, brackets, or braces do not end statements.

Top-level declarations include `module`, `import`, `fn`, `struct`, `enum`, `trait`, `impl`, `test`, and `bench`. Global variables and executable top-level statements are not supported. A function has typed parameters, an optional result type (default `void`), and either an indented body or `=> expression`.

Example (`run:42`):

```nerv
module main
fn add(a: i64, b: i64) -> i64 => a + b
fn main() -> i64
    let answer = add(
        20,
        22
    )
    answer
```

Example (`reject:invalid token`):

```nerv
fn main() -> i64
    let value = 1
  value
```

## 2. Integers and overflow

The alpha core has **one integer representation: signed, two's-complement, 64-bit `i64`**, ranging from -9,223,372,036,854,775,808 through 9,223,372,036,854,775,807. Decimal integer literals use that representation; underscores are ignored within numeric tokens. Out-of-range literals are compile errors, including positive 9,223,372,036,854,775,808. The negative minimum is accepted directly.

For compatibility with existing programs, `i8`, `i16`, `i32`, `u8`, `u16`, `u32`, `u64`, `isize`, and `usize` are currently aliases for `i64`. **They do not provide smaller widths, unsigned arithmetic, or pointer-sized semantics.** Do not use their spelling to infer a layout or range. Distinct widths and signedness require a future language revision and compiler implementation, not just new documentation.

Addition, subtraction, multiplication, and unary negation wrap modulo 2^64 in all build modes. Division truncates toward zero; the remainder has the dividend's sign. `MIN / -1` wraps to `MIN`, and `MIN % -1` is zero. Integer division or remainder by zero is a runtime failure.

Example (`run:-9223372036854775808`):

```nerv
fn main() -> i64 => 9223372036854775807 + 1
```

Example (`run:-9223372036854775808`):

```nerv
fn main() -> i64 => -(-9223372036854775808)
```

Example (`run:0`):

```nerv
fn main() -> i64 => -9223372036854775808 % -1
```

Example (`run:-9223372036854775808`):

```nerv
fn main() -> i64 => -9223372036854775808 / -1
```

Example (`run:-3`):

```nerv
fn main() -> i64 => (-7 / 3) + (-7 % 3)
```

Example (`run:0`):

```nerv
fn main() -> i64
    assert(9223372036854775807 * 2 == -2)
    assert(-9223372036854775808 - 1 == 9223372036854775807)
    0
```

Example (`reject:invalid integer`):

```nerv
fn main() -> i64 => 9223372036854775808
```

## 3. Floating-point values and conversions

`f64` is IEEE 754 binary64; `f32` is currently an alias for it, not binary32. Floating-point literals may contain a decimal point or exponent. A literal that overflows binary64 is rejected. Arithmetic uses IEEE behavior without fast-math: division by zero can produce infinity or NaN. Comparisons with NaN are false except `!=`, which is true. No implicit integer/float conversion occurs in bindings, arithmetic, arguments, or return values.

`f64(integer)` explicitly converts an integer, rounding to the nearest representable binary64 value. `i64(float)` truncates toward zero and fails at runtime for NaN, infinity, or a value outside [-2^63, 2^63). Converting a value to its existing canonical type is an identity operation. Boolean and string conversions are not supported.

Example (`run:42`):

```nerv
fn main() -> i64 => i64(f64(40) + 2.9)
```

Example (`run:-2`):

```nerv
fn main() -> i64 => i64(-2.9)
```

Example (`run:0`):

```nerv
fn main() -> i64
    let nan = 0.0 / 0.0
    assert(nan != nan)
    assert(not (nan == nan))
    assert(not (nan < 1.0))
    0
```

Example (`reject:invalid binary operation`):

```nerv
fn main() -> i64 => 1 + 2.0
```

Example (`reject:expected Float, found Int`):

```nerv
fn main() -> void
    let value: f64 = 1
```

Example (`reject:invalid runtime call`):

```nerv
fn main() -> i64 => i64(true)
```

Example (`reject:float literal is outside the f64 range`):

```nerv
fn main() -> void
    let value = 1e999
```

Example (`fail:float is outside the i64 range`):

```nerv
fn main() -> i64 => i64(9223372036854775808.0)
```

Example (`fail:float is outside the i64 range`):

```nerv
fn main() -> i64 => i64(0.0 / 0.0)
```

## 4. Scope, shadowing, and mutability

Bindings are lexical and visible after their initializer has been evaluated. A new `let` may shadow an earlier binding, including a parameter. The initializer sees the old binding. Each function, conditional branch, loop body, and match arm has its own scope. Inner declarations do not escape their block. Function parameters, range variables, and ordinary `let` bindings cannot be assigned. `let mut` permits assignment of the same type.

Array element and struct field assignments require a mutable named binding. Mutability belongs to the binding, not to the underlying allocation: a mutable array alias can change storage observed through an immutable alias. This is not Rust's borrowing model.

Example (`run:42`):

```nerv
fn main() -> i64
    let value = 40
    let value = value + 2
    value
```

Example (`run:40`):

```nerv
fn main() -> i64
    let value = 40
    let inner = if true
        let value = 2
        value
    else
        0
    value
```

Example (`reject:cannot assign to `):

```nerv
fn main() -> void
    let value = 1
    value = 2
```

Example (`reject:unknown name `):

```nerv
fn main() -> i64
    for i in 0..1
        let hidden = i
    hidden
```

Example (`reject:assignment type mismatch`):

```nerv
fn main() -> void
    let mut value = 1
    value = true
```

Example (`reject:cannot assign to `):

```nerv
fn change(value: i64) -> void
    value = 42
fn main() -> void
    change(0)
```

Example (`reject:cannot assign to `):

```nerv
fn main() -> void
    for i in 0..1
        i = 42
```

## 5. Evaluation and control flow

Operands, call arguments, and array/struct literal elements are evaluated left-to-right, once each. Assignment evaluates the target base and index before the right-hand value; the store occurs after the value has been evaluated. `and` and `or` require booleans and short-circuit. Only the selected `if` branch or `match` arm executes. There are no implicit truth-value conversions.

Precedence, from lowest to highest: `or`; `and`; `== !=`; `< <= > >=`; `+ -`; `* / %`; unary `not -`; calls, indexing, and field/method access. Binary operators at the same level associate left-to-right. Parentheses override precedence.

`for i in start..end` excludes the end; `..=` includes it. Bounds are evaluated once, before iteration. Ranges ascend by one and are empty when the start is greater than the end. An inclusive range ending at `i64`'s maximum stops without overflowing the induction variable. A `while` condition is reevaluated before each iteration.

Example (`run:0`):

```nerv
fn crash() -> bool
    assert(false)
    true
fn main() -> i64
    assert(not (false and crash()))
    assert(true or crash())
    assert(1 + 2 * 3 == 7)
    0
```

Example (`run:12`):

```nerv
fn next(values: []i64) -> i64
    let mut alias = values
    alias[0] = alias[0] + 1
    alias[0]
fn pair(a: i64, b: i64) -> i64 => a * 10 + b
fn main() -> i64
    let values = [0]
    pair(next(values), next(values))
```

Example (`run:7`):

```nerv
fn main() -> i64
    let mut total = 0
    for i in 0..3
        total = total + i
    for i in 9223372036854775807..=9223372036854775807
        total = total + 1
    while total < 7
        total = total + 1
    total
```

Example (`reject:if condition must be bool`):

```nerv
fn main() -> i64
    if 1
        42
    else
        0
```

## 6. Functions, returns, and expression-valued blocks

Functions are declared before checking any bodies, so forward calls and recursion are permitted. Arguments must match the declared parameter count and types. Scalars, structs, and enum values are passed by value. Strings and arrays pass shared handles. Non-capturing function values use `fn(types) -> result`; lambdas cannot access enclosing local bindings.

A non-void function returns its final expression or a top-level `return expression`. A final `if` or `match` is an expression whose branches must have matching types. A block ending in a declaration, assignment, or loop has type `void`; a void function discards its final expression. Void values cannot be bound to variables. In this alpha core, explicit `return` inside a conditional or loop is rejected rather than partially implemented; bare `return` is not supported.

The executable entry point is `main()` returning `i64` or `void`. The current CLI prints an `i64` result, or `0` for a void main, and exits successfully after normal execution. The integer main result is not the process exit status.

Example (`run:42`):

```nerv
fn main() -> i64 => answer()
fn answer() -> i64
    return 42
```

Example (`run:120`):

```nerv
fn factorial(n: i64) -> i64
    if n == 0
        1
    else
        n * factorial(n - 1)
fn main() -> i64 => factorial(5)
```

Example (`run:42`):

```nerv
fn choose(flag: bool) -> i64
    if flag
        42
    else
        0
fn main() -> i64 => choose(true)
```

Example (`run:0`):

```nerv
fn main() -> void
    let mut value = 0
    if true
        value = 42
    else
        value = 1
    assert(value == 42)
```

Example (`reject:wrong argument count`):

```nerv
fn add(a: i64, b: i64) -> i64 => a + b
fn main() -> i64 => add(42)
```

Example (`reject:argument type mismatch`):

```nerv
fn value(a: i64) -> i64 => a
fn main() -> i64 => value(true)
```

Example (`reject:if branches have different types`):

```nerv
fn main() -> i64
    if true
        42
    else
        false
```

Example (`reject:return is only supported directly in a function body`):

```nerv
fn main() -> i64
    if true
        return 42
    else
        0
```

Example (`reject:cannot bind a void value`):

```nerv
fn main() -> void
    let value = assert(true)
```

Example (`run:42`):

```nerv
fn apply(callback: fn(i64) -> i64, value: i64) -> i64 => callback(value)
fn main() -> i64 => apply(fn(value: i64) -> i64 => value + 1, 41)
```

Example (`reject:unknown name `):

```nerv
fn main() -> i64
    let captured = 42
    let callback: fn(i64) -> i64 = fn(value: i64) -> i64 => captured + value
    callback(0)
```

## 7. Modules, visibility, and imports

An import `import lib.math` resolves to `lib/math.nerv` relative to the importing file, not the process's working directory. Resolution is recursive. Canonical paths deduplicate repeated imports, and cycles are compile errors. An import beginning with `nerv` names the built-in runtime and does not load a source file. Built-in runtime calls are also available without such an import.

The optional `module name` declaration is informational in this alpha version; it does not change the file path or establish a namespace. All declarations in the loaded import graph are visible without qualification, including transitive imports. `export fn` is accepted but does not restrict visibility: functions without `export` are also public. Duplicate function names across the graph are rejected. Private declarations and isolated module namespaces are not implemented.

Multi-file example, tested by `module_resolution_visibility_and_cycles`:

`lib/math.nerv`:

```nerv
module math
fn add(a: i64, b: i64) -> i64 => a + b
```

`main.nerv`:

```nerv
import lib.math
fn main() -> i64 => add(20, 22)
```

This evaluates to 42. Changing `lib/math.nerv` to import `main` creates a rejected cycle. Importing a missing file fails compilation; imports do not silently fall back to unrelated files.

## 8. Runtime failures and assertions

`assert(condition)` requires a boolean and returns `void`. A false assertion, integer division/remainder by zero, invalid array index, negative array length, invalid map/reduce length, or out-of-range float-to-integer conversion prints a `runtime error:` diagnostic to standard error and exits with status 1. It does not unwind through native/JIT frames or return a fabricated value. Allocation failure is fatal; array capacity failures use the same runtime diagnostic path.

Array reads and writes check `0 <= index < length`, including negative indices and empty arrays. Map and reduce lengths must lie in `0..=array_length`. Float division by zero follows IEEE arithmetic instead of the integer failure rule.

Example (`fail:assertion failed`):

```nerv
fn main() -> void
    assert(false)
```

Example (`reject:invalid runtime call`):

```nerv
fn main() -> void
    assert(1)
```

Example (`fail:integer division by zero`):

```nerv
fn main() -> i64 => 1 / 0
```

Example (`fail:integer division by zero`):

```nerv
fn main() -> i64 => 1 % 0
```

Example (`fail:array index out of bounds`):

```nerv
fn main() -> i64
    let values = [1]
    values[-1]
```

Example (`fail:array index out of bounds`):

```nerv
fn main() -> void
    let mut values = [1]
    values[1] = 42
```

Example (`fail:array index out of bounds`):

```nerv
fn main() -> i64
    let values: []i64 = []
    values[0]
```

Example (`fail:negative array length`):

```nerv
fn main() -> void
    let values = nerv.arrays.new(-1, 0)
```

Example (`fail:array length out of bounds`):

```nerv
fn add(total: i64, value: i64) -> i64 => total + value
fn main() -> i64
    let values = [1]
    nerv.arrays.reduce(values, 2, 0, add)
```

Example (`fail:invalid array length`):

```nerv
fn identity(value: i64) -> i64 => value
fn main() -> void
    let values = [1]
    let mapped = nerv.arrays.map(values, -1, identity)
```

## 9. Memory ownership, copying, and lifetime

The alpha core uses **execution-region ownership**, not a tracing collector, per-object reference counting, or Rust-style moves and borrowing. Each JIT invocation owns its runtime allocations. Allocations remain alive across function returns and lexical scope exits, and are released together when the invocation finishes normally. Strings and arrays cannot escape into a later invocation. A test/benchmark batch currently shares one execution region for the batch. Fatal process exit lets the operating system reclaim memory.

Integers, floats, booleans, structs, and enum payloads copy by value. A copied struct containing an array or string copies its handle, not its allocation. Array assignment and argument passing share storage; they do not deep-copy. Strings are immutable and can safely share their allocation. There are no user-visible core string/array free operations, and no borrow checker. Programs that continually allocate retain memory until the execution ends; this model is intentionally limited and is not a claim of bounded memory use.

Low-level `nerv.collections` integer handles, raw-pointer access, and explicit frees are experimental native interfaces, not part of the safe core. Forged handles and use-after-free are not supported language operations. A later memory-model milestone can replace these interfaces with typed collections; do not treat them as evidence of memory safety.

Example (`run:42`):

```nerv
fn make() -> []i64
    let values = [0]
    values
fn main() -> i64
    let original = make()
    let mut alias = original
    alias[0] = 42
    original[0]
```

Example (`run:1`):

```nerv
struct Boxed
    value: i64
fn main() -> i64
    let original = Boxed { value: 1 }
    let mut copy = original
    copy.value = 42
    original.value
```

Example (`reject:cannot assign to `):

```nerv
fn main() -> void
    let values = [0]
    values[0] = 42
```

## 10. Strings and arrays

Strings contain UTF-8. Double-quoted literals accept Unicode text and escapes `\n`, `\r`, `\t`, `\"`, and `\\`. Unsupported escapes and literal NUL bytes are rejected. Multiline literals, interpolation, character literals, and embedded NUL escapes are not supported.

`nerv.string.len` counts UTF-8 bytes; `nerv.string.char_count` counts Unicode scalar values, not grapheme clusters. `nerv.string.char_at` uses a scalar-value index and returns its Unicode code point, or zero when absent. String slicing uses byte offsets and returns an empty string for invalid UTF-8 boundaries or reversed bounds, with offsets clamped to the string length. String-producing operations allocate a new immutable string in the execution region.

Core arrays contain only `i64` elements. `[a, b]` allocates elements in source order; `[value] * length` evaluates the initializer once and fills the array. `[]i64` is an array-handle type; the legacy `[i64; N]` annotation currently denotes the same type and does not enforce a static length. Arrays have fixed length after construction. Generic, nested, floating-point, and string arrays are not supported.

For strings and arrays, `==` and `!=` currently compare handle identity, not content. Equal contents in separately created allocations do not imply equality. Scalar comparisons compare values. Fieldless enums support equality within the same enum type. Struct, payload-enum, function, and void equality are rejected; aggregate structural equality is not part of this alpha core.

Example (`run:0`):

```nerv
fn make() -> string => "é🙂\n"
fn main() -> i64
    let text = make()
    assert(nerv.string.len(text) == 7)
    assert(nerv.string.char_count(text) == 3)
    assert(nerv.string.char_at(text, 1) == 128578)
    assert(nerv.string.char_count("\"\\\t\r") == 4)
    0
```

Example (`run:0`):

```nerv
fn main() -> i64
    let original = "Nerv"
    let alias = original
    assert(original == alias)
    assert(original != "Nerv")
    0
```

Example (`run:42`):

```nerv
fn main() -> i64
    let values = [21] * 2
    values[0] + values[1]
```

Example (`run:0`):

```nerv
fn main() -> i64
    let original = [1]
    let alias = original
    assert(original == alias)
    assert(original != [1])
    assert(nerv.string.len(nerv.string.slice("é", 1, 2)) == 0)
    0
```

Example (`reject:equality is not supported for this type`):

```nerv
struct Point
    x: i64
fn main() -> void
    let point = Point { x: 1 }
    assert(point == point)
```

Example (`reject:unsupported string escape`):

```nerv
fn main() -> void
    let text = "\q"
```

Example (`reject:arrays currently require i64 values`):

```nerv
fn main() -> void
    let values = [1.0, 2.0]
```

## Compatibility boundary

This specification deliberately records alpha limitations instead of presenting unimplemented features as complete. True width-specific numeric types, private modules, nested explicit returns, capturing closures, generic arrays, structural aggregate equality, and earlier reclamation of unreachable allocations are separate changes. Any revision must update the examples and conformance tests along with the compiler.
