# Standard library

Use `import nerv.core` for `Option<T>`, `Result<T, E>`, `option_or(value, fallback)`, and `result_or(value, fallback)`. Use `import nerv.std` for the complete small library. Sources are embedded in the compiler, so builds do not depend on a working-directory library installation. Imports are deduplicated. Names follow the current language rule of global import visibility.

Recoverable operations return ordinary result enums. Handle both `Ok(value)` and `Err(error)` in a match, or supply a default with `result_or`. `IoError` contains `operation`, `path`, and `message`. Allocation failure, failed assertions, invalid indices, and invalid arithmetic remain fatal runtime errors. See [language rules](language-spec.md).

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
