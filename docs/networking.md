# JSON, HTTPS, and WebSockets

These APIs are implemented in the current source tree. Rebuild the compiler before using them: earlier `0.1.0-alpha.1` release archives do not include these additions. The runtime uses serde_json, reqwest, rustls/ring, and tungstenite; the resulting Nerv application links that support natively and needs no helper process, Rust compiler, Cargo, or LLVM at execution time. Discord-specific protocol logic belongs in a separate Nerv library.

## JSON

`import nerv.json` loads the JSON wrappers and standard library. `Json` is an ordinary struct with a `text: string` field containing serialized JSON; `JsonError` has a `message: string` field. Prefer constructors and `json_parse` over manually wrapping unvalidated text.

| API | Result |
| --- | --- |
| `json_parse(text: string)` | `Result<Json, JsonError>`; validates and serializes input. |
| `json_get(value: Json, key: string)` | `Result<Json, JsonError>`; object property lookup; missing keys are errors. |
| `json_index(value: Json, index: i64)` | `Result<Json, JsonError>`; checked array access. |
| `json_kind(value: Json)` | `string`: `object`, `array`, `string`, `number`, `boolean`, `null`, or `invalid`. |
| `json_text(value: Json, fallback: string)` | Decoded string or the explicit fallback. |
| `json_i64(value: Json, fallback: i64)` | Exact signed integer or fallback; no float/string coercion. |
| `json_f64(value: Json, fallback: f64)` | Finite floating-point value or fallback; may round large numbers. |
| `json_bool(value: Json, fallback: bool)` | Boolean or fallback. |
| `json_len(value: Json)` | Array/object length as `i64`; zero for other/invalid values. |
| `json_string(value: string)` | `Json`; escapes a UTF-8 string. |
| `json_integer(value: i64)` | `Json`; serializes an integer. |
| `json_number(value: f64)` | `Result<Json, JsonError>`; rejects NaN/infinity. |
| `json_boolean(value: bool)`, `json_null()` | `Json`. |
| `json_object()`, `json_array()` | Empty object/array `Json`. |
| `json_set(value: Json, key: string, member: Json)` | `Result<Json, JsonError>`; copies/replaces an object member. |
| `json_push(value: Json, member: Json)` | `Result<Json, JsonError>`; copies/appends to an array. |
| `json_serialize(value: Json)` | The stored JSON text as `string`. |

Parsing and fallible builders accept at most 4 MiB of JSON and use serde_json's normal recursion limit (128). Arbitrary-precision number text is retained instead of converting every number to a float. Keep Discord snowflake IDs as strings. Duplicate object keys use the last value. Whitespace/member ordering can change; output is not a cryptographic canonicalization format. `json_serialize` returns the wrapper's text without revalidating it. String/integer/boolean constructors produce JSON directly; oversized strings will be rejected by later bounded parsing/building operations.

This first interface reparses serialized values during operations and returns copied JSON. It is intended for small payloads and application state, rather than streaming large documents. `example/json.nerv` is a runnable example and is covered in JIT and both native build modes.

## HTTP and HTTPS clients

`import nerv.net` loads networking, JSON, and the standard library.

```text
http_request(method: string, url: string, headers: []string, body: string, timeout_ms: i64)
    -> Result<HttpResponse, NetError>
http_request_with_ca(method: string, url: string, headers: []string, body: string, timeout_ms: i64, ca_file: string)
    -> Result<HttpResponse, NetError>
http_header(response: HttpResponse, name: string) -> Option<string>
```

`headers` contains alternating names and values, such as `["Authorization", "Bot TOKEN", "Content-Type", "application/json"]`. Use `[""] * 0` for an empty string array. `HttpResponse` contains `status: i64`, `headers: Json` (a flat array of alternating names/values), and `body: string`. `NetError` contains `operation: string` and `message: string`. Header lookup ignores ASCII case and returns the last matching value.

Transport/configuration errors produce `Err`; HTTP statuses, including 401, 429, and 500, produce `Ok(HttpResponse)` so applications can inspect the status, headers, and body. Redirects are returned as responses and are not followed automatically. Only `http` and `https` schemes are accepted. Request/response bodies are UTF-8 text, bounded to 4 MiB; arbitrary binary HTTP downloads and automatic content decompression are not exposed by this interface. Timeouts must be 1..300000 milliseconds. Response collection is bounded even when Content-Length is missing.

TLS verifies server identity and certificate chains against bundled Mozilla roots. `_with_ca` adds certificates from an explicit PEM file and retains normal hostname/expiry verification. There is no insecure certificate-verification switch. The APIs do not invoke a shell or external networking program. HTTP error formatting removes the request URL and preserves useful underlying transport errors, but application code must still avoid logging tokens or complete authorization headers.

## WebSocket clients

```text
websocket_run(url: string, state: string, read_timeout_ms: i64, handler: fn(i64, string, string, []u8, string) -> []string)
    -> WebSocketOutcome
websocket_run_with_options(url: string, headers: []string, state: string, read_timeout_ms: i64, ca_file: string, handler: fn(i64, string, string, []u8, string) -> []string)
    -> WebSocketOutcome
websocket_send(handle: i64, message: string) -> Result<bool, NetError>
websocket_send_binary(handle: i64, bytes: []u8) -> Result<bool, NetError>
websocket_close(handle: i64) -> Result<bool, NetError>
websocket_close_with_code(handle: i64, code: i64, reason: string) -> Result<bool, NetError>
```

`ws` and `wss` are supported. Use `wss` for Discord. Extra handshake headers use the same flat array convention as HTTP; the transport manages Host, Upgrade, Connection, and WebSocket key/version/extensions headers. WebSocket compression is not implemented; do not request compressed Gateway payloads or compression extensions.

The handler receives `(handle, kind, text, bytes, state)` and returns exactly `[action, next_state]`, where action is `"continue"` or `"close"`. The state is an explicit UTF-8 string; JSON is a useful application-state encoding. Callback function values are noncapturing, as elsewhere in Nerv. `WebSocketOutcome` has `state: string` and `error: string`; empty error means the transport loop completed without a transport failure. A peer close code can still signal an application error, so a Discord library must inspect the close event and save its classification in state. It retains the last successfully returned application state on a transport failure.

| Event kind | Data |
| --- | --- |
| `open` | Connection/handshake complete; empty text/bytes. |
| `text` | UTF-8 message in `text`. |
| `binary` | Message data in `bytes: []u8`. |
| `ping`, `pong` | Control-frame payload in `bytes`; transport replies to ping automatically. |
| `timeout` | No complete message before the configured read timeout; not a disconnection. |
| `close` | `text` contains JSON `{code, reason}`, or `null` if no code is available. |
| `error` | Transport error text; the loop terminates after this callback. |

Connection/TLS/WebSocket handshake and writes use ten-second timeouts; address connection attempts share a ten-second budget. OS DNS resolution may take longer than that budget. Reads use the supplied 1..300000 millisecond timeout. Messages/frames and returned state are bounded to 4 MiB. Close reasons are at most 123 UTF-8 bytes, and reserved/invalid close codes are rejected. Default close sends an empty close payload; use `_with_code` when a protocol requires a particular code. Reconnect/Identify/Resume policy is application code, not transport behavior.

Every callback executes in a nested allocation region. Its temporary strings/arrays/JSON values are released when it returns. Only the copied state string crosses event boundaries. Connection IDs are checked registry keys, not raw pointers, and become invalid when the loop returns. Do not retain an ID for use after the loop. Callbacks may send HTTP requests or WebSocket messages and may request a close. Keep callbacks short: the loop is synchronous and a long callback delays all reads, heartbeat processing, and other events on that connection. There is no async/task/thread API or built-in sharding layer.

`monotonic_ms() -> i64` measures milliseconds since the process's first call and is suitable for interval deadlines; `now_ms()` remains wall-clock epoch time. A Discord library must check heartbeat deadlines on incoming events as well as `timeout` events, since a busy connection need not time out.

## Verification and runtime builds

`cargo test --locked -p nerv --test networking` covers JSON, HTTPS status/headers/body, WSS text/binary/control frames, custom close codes, certificate rejection, and callback type rejection. Successful network cases run in JIT and debug/release standalone modes against local TLS peers using generated certificates. No Discord token or live Discord traffic is used. Runtime tests cover callback-region cleanup, stale handles, idle timeouts, response limits, and invalid parameters.

The CLI build script builds the static runtime through Cargo in a separate OUT_DIR target directory, embedding dependencies and native link-library information. It uses the repository lockfile and offline dependencies already fetched by the outer build. Runtime networking is a default Cargo feature. `tools/check-memory.sh` disables that feature for Miri's core memory tests because OS socket/TLS operations are not supported by Miri; normal native tests exercise networking. Networking tests have been verified locally on Windows; other platforms still require their own native test run for these new changes.
