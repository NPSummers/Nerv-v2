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

Integers have distinct widths and signedness: `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, and `u64`. `isize` and `usize` use the host pointer width. Unsuffixed integer literals have type `i64`; use explicit conversions to construct other widths. Out-of-range i64 literals are rejected, except that the negative minimum is accepted directly.

Addition, subtraction, multiplication, and unary negation wrap at the operand width in every build mode. Signed division truncates toward zero and signed remainder has the dividend's sign. Unsigned division and comparisons interpret the bit pattern as nonnegative. Signed `MIN / -1` wraps to `MIN`; `MIN % -1` is zero. Division or remainder by zero fails at runtime.

Integer conversions truncate high bits when narrowing and sign-extend or zero-extend according to the source type when widening. Same-width conversions preserve bits. Mixed numeric types require explicit conversion in arithmetic, bindings, arguments, and returns.

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

`f64` is IEEE 754 binary64 and `f32` is binary32. Unsuffixed floating literals have type `f64`; `f32(value)` rounds to binary32. Arithmetic uses IEEE behavior without fast-math. NaN comparisons are false except `!=`. Integer-to-float conversions respect signedness and round to the destination precision. Float widening preserves the value; narrowing rounds to binary32.

Every numeric type name is an explicit conversion function. Float-to-integer conversions truncate toward zero and fail for NaN, infinity, or inputs outside the destination range (inclusive lower bound, exclusive upper bound). `i64(float)` retains the diagnostic `float is outside the i64 range`. Boolean and string conversions are unsupported.

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

Functions are declared before checking any bodies, so forward calls and recursion are permitted. Arguments must match the declared parameter count and types. Scalars, structs, and enum values are passed by value. Strings and arrays pass shared handles. Function values use `fn(types) -> result`. Lambdas capture referenced outer bindings by value when created; string and array handles retain their normal sharing semantics. Captured bindings cannot be rebound inside the lambda.

A non-void function returns its final expression or `return expression`, including from nested branches and loops. A final `if` or `match` is an expression whose branches must have matching types. A block ending in a declaration, assignment, or loop has type `void`; a void function discards its final expression. Void values cannot be bound to variables. Bare `return` exits a void function. `break` exits the innermost loop and `continue` advances it. Both are rejected outside loops. An `if` without `else` has an empty else branch; value-producing branches must still agree on type, except branches that exit control flow.

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

Example (`run:42`):

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

Example (`run:42`):

```nerv
fn main() -> i64
    let captured = 42
    let callback: fn(i64) -> i64 = fn(value: i64) -> i64 => captured + value
    callback(0)
```

## 7. Modules, visibility, and imports

An import `import lib.math` resolves to `lib/math.nerv` relative to the importing file, not the process's working directory. Resolution is recursive. Canonical paths deduplicate repeated imports, and cycles are compile errors. An import beginning with `nerv` names the built-in runtime and does not load a source file. Built-in runtime calls are also available without such an import.

Imported declarations can be qualified by their import path, including functions, constructors, and types: `lib.math.answer()` or `lib.models.Value`. Names from different modules may collide; qualified references select the defining module, while unqualified references prefer local declarations and must resolve without ambiguity among imports. Unique legacy unqualified names remain supported, including transitive imports. A `module name` declaration supplies an additional qualified name without changing path resolution. `export fn` remains accepted; functions without `export` are also public. Private declarations are not implemented.

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

The alpha core uses **execution-region ownership**, not a tracing collector, per-object reference counting, or Rust-style moves and borrowing. Each JIT invocation or standalone entry owns its runtime allocations. Allocations remain alive across function returns and lexical scope exits, and are released together when the invocation finishes normally. Strings and arrays cannot escape into a later invocation. `allocation_scope` reclaims a nested region after a noncapturing integer worker returns; tasks run in separate regions with scalar inputs/results and cooperative cancellation. See the standard-library documentation for these APIs and their restrictions. A test/benchmark batch currently shares one execution region for the batch. Fatal process exit lets the operating system reclaim memory. See the [runtime ABI](runtime-abi.md) and [standalone compilation](standalone.md) documents for implementation and validation details.

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

Core arrays contain elements of any single non-void type, including structs, enums, function values, and nested arrays. `[a, b]` allocates elements in source order; `[value] * length` evaluates the initializer once and fills the array. `[]T` is an array-handle type; the legacy `[i64; N]` annotation currently denotes the same type and does not enforce a static length. Arrays have fixed length after construction. Empty array literals default to `[]i64`.

For strings, `==` and `!=` compare UTF-8 contents. For arrays, they compare handle identity; equal contents in separately created arrays do not imply equality. Scalar comparisons compare values. Fieldless enums support equality within the same enum type. Struct, payload-enum, function, and void equality are rejected; aggregate structural equality is not part of this alpha core.

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
    assert(original == "Nerv")
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

Example (`run:0`):

```nerv
fn main() -> void
    let values = [1.0, 2.0]
```

## Compatibility boundary

This specification deliberately records alpha limitations instead of presenting unimplemented features as complete. Private modules, nested explicit returns, capturing closures, structural aggregate equality, and earlier reclamation of unreachable allocations are separate changes. Any revision must update the examples and conformance tests along with the compiler.

## 12. Generics and recoverable errors

Functions, structs, and enums declare parameters with `<T, E>`. Type arguments in annotations use `Box<i64>` or `Result<i64, string>`. Calls infer arguments from values and the immediate binding or return annotation. Inference does not search through arbitrary surrounding calls: bind an otherwise ambiguous fieldless constructor to an annotated variable first. Conflicting arguments and uninferred parameters are errors. Each concrete instantiation is compiled separately, with a limit of 256 specializations per program to diagnose expanding recursion.

A parameter may require one trait, as in `fn read<T: Read>(value: T) -> i64 => value:read()`. The concrete type must declare that implementation. Generic struct methods inherit the struct's parameters. Unused generic bodies are checked when instantiated. Generic method declarations and generic trait declarations are not supported.

Optional and result values are ordinary enums; use exhaustive `match` to supply defaults or propagate errors. Propagation is an explicit match whose error arm constructs the enclosing function's error variant. `value?` unwraps `Ok`/`Some` and returns `Err`/`None` from the enclosing function. Result propagation requires an identical error type; it may change the success type. Option propagation requires an Option return type. Runtime violations and allocation failure remain unrecoverable; recoverable application errors are values.

Example (`run:42`):

```nerv
enum Option<T>
    Some(T)
    None
fn fallback<T>(value: Option<T>, other: T) -> T
    match value
        Some(item) => item
        None => other
fn main() -> i64 => fallback(Some(42), 0)
```

Example (`reject:conflicting types`):

```nerv
fn first<T>(a: T, b: T) -> T => a
fn main() -> i64 => first(1, true)
```
