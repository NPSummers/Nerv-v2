# Standard library

Use `import nerv.core` for `Option<T>`, `Result<T, E>`, `option_or(value, fallback)`, and `result_or(value, fallback)`. Use `import nerv.std` for the complete small library. Sources are embedded in the compiler, so builds do not depend on a working-directory library installation. Imports are deduplicated. Imports support qualified module names; unique legacy unqualified names remain available. `nerv.text`, `nerv.path`, `nerv.fs`, `nerv.task`, and `nerv.memory` also load the ordinary standard-library helpers.

Recoverable operations return ordinary result enums. Handle both `Ok(value)` and `Err(error)` in a match, or supply a default with `result_or`. `IoError` contains `kind`, `operation`, `path`, and `message`. Common kinds include `not_found`, `permission_denied`, `already_exists`, `invalid_input`, `invalid_data`, `not_a_directory`, `is_a_directory`, and `directory_not_empty`; unclassified errors use `other`. Allocation failure, failed assertions, invalid indices, and invalid arithmetic remain fatal runtime errors. See [language rules](language-spec.md).

## Collections

| API | Behavior |
| --- | --- |
| `vector(seed)` | Empty `Vector<T>`; seed establishes the element type. |
| `v:push(value)` | Returns a new vector containing the appended value. |
| `v:len()`, `v:get(index)` | Length and checked element access. |
| `v:each(callback)` | Visits elements in order; callback has type `fn(T) -> void`. |
| `map(seed)` | Empty `Map<T>` with UTF-8 string keys. |
| `m:insert(key, value)` | Returns a new map; replaces an existing key. |
| `m:get(key)` | `Option<T>`; compares key contents. |
| `m:len()` | Number of keys. |
| `set()` | Empty string `Set`. |
| `s:insert(key)` | Returns a new set; repeated keys are ignored. |
| `s:contains(key)`, `s:len()` | Membership and size. |
| `nerv.arrays.len(array)` | Fixed-array length for any element type. |

Assign the returned collection to retain updates, for example `values = values:push(item)`. Collections copy their buffers when updated; existing snapshots retain their elements. Element handles remain shared according to execution-region ownership. Vector append and map insertion copy existing elements; map lookup and set membership scan linearly. These APIs are intended for small applications, not high-volume collection benchmarks. Arrays remain fixed-size; vector updates allocate new arrays.

## Input and output

| API | Result |
| --- | --- |
| `read_text(path)` | `Result<string, IoError>`; rejects non-UTF-8 files. |
| `write_text(path, contents)` | `Result<i64, IoError>` with UTF-8 byte count; creates or truncates a file. |
| `create_directory(path)` | `Result<i64, IoError>`; creates parents, accepts an existing directory. |
| `list_directory(path)` | `Result<[]string, IoError>` with sorted entry names. |
| `read_stdin()` | `Result<string, IoError>`; reads UTF-8 until EOF. |
| `print(value)`, `println(value)` | Standard output for numeric, boolean, and string values. |
| `stderr(message)` | Writes a string to standard error without adding a newline. |
| `arguments()` | Argument strings including executable name. |
| `environment(name)` | `Option<string>`; missing and non-Unicode values produce `None`. |

Paths are relative to the process working directory. Directory names and process arguments that cannot be represented as Unicode use replacement characters. Project applications and `nerv run` receive their own process arguments, with options after `--` forwarded unchanged. Historical JIT `--run` execution exposes the compiler process arguments. See [project tooling](projects.md).

## Strings, time, random values, and tests

`string_equal(a, b)` compares UTF-8 contents. Existing runtime APIs remain available: `nerv.string.len`, `concat`, `is_empty`, `contains`, `starts_with`, `ends_with`, `to_upper`, `to_lower`, `trim`, `replace`, `repeat`, `slice`, `find`, `char_count`, and `char_at`. Length and slice offsets count bytes; `char_count` and `char_at` operate on Unicode scalar values, not grapheme clusters. Predicates in the existing `nerv.string` namespace return integer 0 or 1. `split` and `split_once` retain their legacy string-returning behavior and are not collection APIs.

`now_ms()` returns milliseconds since the Unix epoch; it is wall-clock time and can move backward. `duration_ms(n)` constructs a nonnegative `Duration`; `sleep(duration)` and `sleep_ms(n)` wait for that duration. `random_below(n)` returns a value in `0..n` for positive `n`; the existing generator is deterministic and unsuitable for cryptography. `assert`, `assert_equal(actual, expected)`, and `assert_string_equal(actual, expected)` abort on failure. Generic equality assertions support types with language equality; strings use the dedicated content assertion. Source `test` blocks remain available through the existing CLI test runner.

## Runnable examples

Run `./nerv.ps1 example/standard-library.nerv` on Windows, or `./nerv.sh example/standard-library.nerv` on macOS. It exercises vector, map, set, time, and arguments and returns 42. Compile `example/file-processing.nerv` with `nerv build` and pass an input and output path to trim a UTF-8 file into another file. `example/generics.nerv`, `example/numeric-types.nerv`, and `example/enum-payloads.nerv` exercise the core language additions. Compiler tests execute these features through JIT and debug/release standalone binaries; I/O tests use temporary directories and piped standard input.

## JSON and network clients

`import nerv.json` and `import nerv.net` add typed JSON helpers, HTTP/HTTPS clients, and WebSocket event callbacks. They include the ordinary standard library transitively. See [networking](networking.md) for signatures, timeouts, TLS verification, callback state, and allocation lifetimes. These are source-tree additions requiring a rebuilt compiler.

## Text conversion and formatting

- `integer_text(i64)`, `float_text(f64)`, and `boolean_text(bool)` convert values without JSON serialization.
- `parse_integer(string) -> Result<i64, ParseError>` checks syntax and range. `parse_float(string) -> Result<f64, ParseError>` rejects invalid and non-finite values. Parsing does not trim whitespace.
- `split_text(text, separator) -> []string` preserves empty pieces and supports Unicode delimiters; an empty delimiter follows Unicode scalar boundaries and includes leading/trailing empty pieces.
- `text_lines(text) -> []string` handles LF/CRLF and omits the trailing empty line. `join_text(values, separator)` joins a string array.
- `format_text(template, values) -> Result<string, FormatError>` replaces `{}` in order and accepts `{{`/`}}` for literal braces. Invalid braces and missing/extra arguments are errors. Convert numeric arguments explicitly. Indexed fields and formatting specifications are not supported.

String `==` and `!=` compare UTF-8 contents. Array equality remains handle identity.

## Paths and file operations

`path(text)` constructs a `Path` with `text`, `join(child)`, `parent()`, `file_name()`, `extension()`, `is_absolute()`, and `canonicalize()` methods. Joining uses host-platform path rules; an absolute child can replace the base. Lexical operations do not access the filesystem. An absent filename/extension/parent returns an empty string/path. `canonicalize()` requires the path to exist and resolves links using the host platform.

`current_directory()` returns `Result<Path, IoError>`. `path_exists(path)` returns a fallible boolean so permission errors are not confused with absence. `rename_path(source, destination)`, `remove_file(path)`, and `remove_directory(path)` return `Result<bool, IoError>`. Directory removal is nonrecursive. Rename/overwrite behavior follows the host OS. `file_metadata(path)` returns `FileMetadata` with byte `size`, `is_file`, `is_directory`, and epoch `modified_ms` (`-1` when unavailable). Metadata follows symbolic links.

Adding `IoError.kind` changes source struct construction: callers constructing errors must now supply the field. Existing compiled standalone executables retain their embedded runtime.

## Growing buffers and hash maps

`buffer(seed) -> Buffer<T>` provides `len`, `capacity`, `get`, `reserve`, `push`, `pop`, and `clear`. Assign returned buffers to retain their length/capacity updates. Appending grows storage geometrically, giving amortized constant-time append. `pop` reduces length without returning the removed element; read it with `get` first if needed. `clear` retains storage.

`hash_map(seed) -> HashMap<T>` provides string-keyed `get`, `insert`, `remove`, `len`, and `capacity`. It uses a process-randomized string hash, open addressing, tombstones, and geometric rehashing. Lookup/update have expected constant-time cost, with linear worst cases. Hash values are not stable serialization identifiers.

These containers share their backing storage when copied. They are intended to be updated through one logical owner; copies are aliases, not persistent snapshots. Retain the existing `Vector`, `Map`, and `Set` for snapshot semantics. Old backing arrays remain allocated until their execution region ends, but geometric growth bounds the sum of retained backing capacities to linear rather than quadratic growth.

## Allocation scopes

`allocation_scope(input: i64, worker: fn(i64) -> i64) -> i64` executes a noncapturing worker in a nested allocation region. Temporary strings, arrays, closures, and containers are reclaimed when it returns. Only its integer result escapes. This supports repeated allocation-heavy work with bounded temporary memory. Pass data through the integer input; captured workers are rejected at runtime. Noncapturing function values use static closure descriptors and do not allocate repeatedly in the outer region.

Ordinary function/lexical scope exits still do not automatically reclaim allocations. This API is explicit region management, not a tracing garbage collector or general resource ownership system. Fatal failures still terminate the process.

## Tasks and cancellation

`spawn_task(input: i64, worker: fn(i64) -> i64) -> Result<Task, TaskError>` starts a native OS thread with its own allocation region. Workers must be noncapturing; runtime validation rejects captured environments. This keeps mutable managed handles from crossing threads. `join_task(task) -> Result<i64, TaskError>` consumes the registry handle and waits for its result. Joining twice is an error.

`cancel_task(task) -> bool` requests cancellation, and `cancellation_requested() -> bool` lets the worker observe it. Cancellation is cooperative: workers must poll and return. Region teardown requests cancellation and joins any unfinished tasks created in that region, so work cannot outlive its code or region. A worker that ignores cancellation can delay teardown indefinitely. Task handles are scoped to the spawning thread. Async/await, shared-memory channels, arbitrary captured tasks, and forced termination are not provided.

Executable examples: `example/foundation.nerv`, `example/control-flow.nerv`, `example/propagation.nerv`, `example/closures.nerv`, `example/namespaces/main.nerv`, `example/containers.nerv`, and `example/tasks.nerv`. Integration tests run each through JIT and native debug/release builds.

Local verification on 2026-11-06 passed all 204 workspace tests, plus 21 runtime tests with networking disabled. New executable examples passed JIT and debug/release native checks on Windows. The sibling Discord SDK/bot tests and native mock transport checks also passed. Miri validation was unavailable in the installed nightly toolchain; native tests are not a substitute for that check. Other host platforms still need native verification.
