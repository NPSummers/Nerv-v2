use super::{
    ArrayHeader, Execution, NervString, RuntimeSymbol, TypedArray, nerv_array_new,
    standard::{strings, text},
    string,
};
use reqwest::{
    Url,
    blocking::Client,
    header::{HeaderName, HeaderValue},
};
use rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, pem::PemObject},
};
use std::{
    cell::RefCell,
    collections::HashMap,
    ffi::c_void,
    io::{self, Read},
    net::TcpStream,
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
    time::{Duration, Instant},
};
use tungstenite::{
    Connector, Message, WebSocket, client::IntoClientRequest, client_tls_with_config,
    protocol::WebSocketConfig, stream::MaybeTlsStream,
};

const LIMIT: usize = 4 * 1024 * 1024;
type Socket = WebSocket<MaybeTlsStream<TcpStream>>;
pub type Callback = unsafe extern "C" fn(
    i64,
    *const NervString,
    *const NervString,
    *const c_void,
    *const NervString,
) -> *const ArrayHeader;
static NEXT_SOCKET: AtomicI64 = AtomicI64::new(1);
thread_local! { static SOCKETS: RefCell<HashMap<i64, Socket>> = RefCell::new(HashMap::new()); }

struct Lease(i64);
impl Drop for Lease {
    fn drop(&mut self) {
        SOCKETS.with(|s| {
            s.borrow_mut().remove(&self.0);
        });
    }
}

fn duration(milliseconds: i64) -> Result<Duration, String> {
    if !(1..=300_000).contains(&milliseconds) {
        return Err("timeout must be between 1 and 300000 milliseconds".into());
    }
    Ok(Duration::from_millis(milliseconds as u64))
}

fn tls_config(ca_file: &str) -> Result<ClientConfig, String> {
    let mut roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    if !ca_file.is_empty() {
        let data = std::fs::read(ca_file).map_err(|e| format!("could not read CA file: {e}"))?;
        let mut count = 0;
        for certificate in CertificateDer::pem_slice_iter(&data) {
            roots
                .add(certificate.map_err(|e| format!("invalid CA certificate: {e}"))?)
                .map_err(|e| format!("invalid CA certificate: {e}"))?;
            count += 1;
        }
        if count == 0 {
            return Err("CA file contains no certificates".into());
        }
    }
    Ok(
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_root_certificates(roots)
            .with_no_client_auth(),
    )
}

unsafe fn string_array(array: *const ArrayHeader) -> Result<Vec<String>, String> {
    let array = unsafe { array.as_ref() }.ok_or("invalid string array")?;
    let length = usize::try_from(array.length).map_err(|_| "invalid string array length")?;
    if length > 4096 {
        return Err("string array exceeds 4096 entries".into());
    }
    let mut values = Vec::with_capacity(length);
    for index in 0..length {
        let pointer = unsafe { array.data.cast::<*const NervString>().add(index).read() };
        values.push(unsafe { text(pointer) }.to_owned());
    }
    Ok(values)
}

fn request_error(error: reqwest::Error) -> String {
    use std::error::Error;
    let error = error.without_url();
    let mut message = error.to_string();
    let mut cause = error.source();
    while let Some(error) = cause {
        let detail = error.to_string();
        if !message.ends_with(&detail) {
            message.push_str(": ");
            message.push_str(&detail);
        }
        cause = error.source();
    }
    message
}

fn http_request(
    method: &str,
    url: &str,
    headers: &[String],
    body: &str,
    timeout: i64,
    ca_file: &str,
) -> Result<Vec<String>, String> {
    let url = Url::parse(url).map_err(|_| "invalid HTTP URL")?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("HTTP URLs must use http or https".into());
    }
    if headers.len() % 2 != 0 {
        return Err("HTTP headers must contain alternating names and values".into());
    }
    if body.len() > LIMIT {
        return Err("HTTP body exceeds 4 MiB".into());
    }
    let method = method
        .parse::<reqwest::Method>()
        .map_err(|_| "invalid HTTP method")?;
    let timeout = duration(timeout)?;
    let client = Client::builder()
        .use_preconfigured_tls(tls_config(ca_file)?)
        .timeout(timeout)
        .connect_timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(request_error)?;
    let mut request = client.request(method, url).body(body.to_owned());
    for pair in headers.chunks_exact(2) {
        let name =
            HeaderName::from_bytes(pair[0].as_bytes()).map_err(|_| "invalid HTTP header name")?;
        let value = HeaderValue::from_str(&pair[1]).map_err(|_| "invalid HTTP header value")?;
        request = request.header(name, value);
    }
    let response = request.send().map_err(request_error)?;
    if response.content_length().is_some_and(|n| n > LIMIT as u64) {
        return Err("HTTP response exceeds 4 MiB".into());
    }
    let status = response.status().as_u16().to_string();
    let mut response_headers = Vec::new();
    for (name, value) in response.headers() {
        response_headers.push(name.as_str().to_owned());
        response_headers.push(
            value
                .to_str()
                .map_err(|_| "HTTP response header is not UTF-8")?
                .to_owned(),
        );
    }
    let mut bytes = Vec::new();
    response
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > LIMIT {
        return Err("HTTP response exceeds 4 MiB".into());
    }
    let body = String::from_utf8(bytes).map_err(|_| "HTTP response body is not UTF-8")?;
    Ok(vec![
        status,
        serde_json::to_string(&response_headers).map_err(|e| e.to_string())?,
        body,
        String::new(),
    ])
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_net_http_request(
    method: *const NervString,
    url: *const NervString,
    headers: *const ArrayHeader,
    body: *const NervString,
    timeout: i64,
    ca_file: *const NervString,
) -> *mut c_void {
    let result = unsafe { string_array(headers) }.and_then(|headers| {
        http_request(
            unsafe { text(method) },
            unsafe { text(url) },
            &headers,
            unsafe { text(body) },
            timeout,
            unsafe { text(ca_file) },
        )
    });
    strings(result.unwrap_or_else(|e| vec!["0".into(), "[]".into(), String::new(), e]))
}

fn open(
    url: &str,
    headers: &[String],
    read_timeout: Duration,
    ca_file: &str,
) -> Result<Socket, String> {
    let timeout = Duration::from_secs(10);
    let parsed = Url::parse(url).map_err(|_| "invalid WebSocket URL")?;
    if !matches!(parsed.scheme(), "ws" | "wss") {
        return Err("WebSocket URLs must use ws or wss".into());
    }
    if parsed.host_str().is_none() {
        return Err("WebSocket URL needs a host".into());
    }
    let addresses = parsed.socket_addrs(|| None).map_err(|e| e.to_string())?;
    let start = Instant::now();
    let mut connected = None;
    let mut last_error = "no addresses found".to_owned();
    for address in addresses {
        let remaining = timeout
            .checked_sub(start.elapsed())
            .ok_or("WebSocket connect timed out")?;
        match TcpStream::connect_timeout(&address, remaining) {
            Ok(stream) => {
                connected = Some(stream);
                break;
            }
            Err(error) => last_error = error.to_string(),
        }
    }
    let stream = connected.ok_or(last_error)?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|e| e.to_string())?;
    let config = WebSocketConfig::default()
        .max_message_size(Some(LIMIT))
        .max_frame_size(Some(LIMIT))
        .max_write_buffer_size(LIMIT * 2);
    let mut request = url.into_client_request().map_err(|e| e.to_string())?;
    if headers.len() % 2 != 0 {
        return Err("WebSocket headers must contain alternating names and values".into());
    }
    for pair in headers.chunks_exact(2) {
        let name = HeaderName::from_bytes(pair[0].as_bytes())
            .map_err(|_| "invalid WebSocket header name")?;
        if matches!(
            name.as_str(),
            "host"
                | "connection"
                | "upgrade"
                | "sec-websocket-key"
                | "sec-websocket-version"
                | "sec-websocket-extensions"
        ) {
            return Err("WebSocket handshake headers are managed by the transport".into());
        }
        let value =
            HeaderValue::from_str(&pair[1]).map_err(|_| "invalid WebSocket header value")?;
        request.headers_mut().append(name, value);
    }
    let (mut socket, _) = client_tls_with_config(
        request,
        stream,
        Some(config),
        Some(Connector::Rustls(Arc::new(tls_config(ca_file)?))),
    )
    .map_err(|e| e.to_string())?;
    let stream = match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => stream,
        MaybeTlsStream::Rustls(stream) => &mut stream.sock,
        _ => return Err("unsupported WebSocket TLS backend".into()),
    };
    stream
        .set_read_timeout(Some(read_timeout))
        .map_err(|e| e.to_string())?;
    Ok(socket)
}

fn with_socket<T>(
    handle: i64,
    operation: impl FnOnce(&mut Socket) -> Result<T, tungstenite::Error>,
) -> Result<T, String> {
    SOCKETS.with(|sockets| {
        let mut sockets = sockets.try_borrow_mut().map_err(|_| "WebSocket is busy")?;
        operation(
            sockets
                .get_mut(&handle)
                .ok_or("WebSocket handle is closed or invalid")?,
        )
        .map_err(|e| e.to_string())
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_net_websocket_send(
    handle: i64,
    value: *const NervString,
) -> *mut NervString {
    let value = unsafe { text(value) };
    let error = if value.len() > LIMIT {
        Some("WebSocket message exceeds 4 MiB".to_owned())
    } else {
        with_socket(handle, |socket| {
            socket.send(Message::Text(value.to_owned().into()))
        })
        .err()
    };
    string(error.unwrap_or_default())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_net_websocket_send_binary(
    handle: i64,
    array: *const ArrayHeader,
) -> *mut NervString {
    let result = (|| {
        let array = unsafe { array.as_ref() }.ok_or("invalid byte array")?;
        let length = usize::try_from(array.length).map_err(|_| "invalid byte array length")?;
        if length > LIMIT {
            return Err("WebSocket message exceeds 4 MiB".into());
        }
        let bytes = if length == 0 {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(array.data.cast::<u8>(), length) }.to_vec()
        };
        with_socket(handle, |socket| socket.send(Message::Binary(bytes.into())))
    })();
    string(result.err().unwrap_or_default())
}

#[unsafe(no_mangle)]
pub extern "C" fn nerv_net_websocket_close(handle: i64) -> *mut NervString {
    string(
        with_socket(handle, |socket| socket.close(None))
            .err()
            .unwrap_or_default(),
    )
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_net_websocket_close_with_code(
    handle: i64,
    code: i64,
    reason: *const NervString,
) -> *mut NervString {
    let result = (|| {
        let code = u16::try_from(code).map_err(|_| "invalid WebSocket close code")?;
        let code = tungstenite::protocol::frame::coding::CloseCode::from(code);
        if !code.is_allowed() {
            return Err("invalid WebSocket close code".into());
        }
        let reason = unsafe { text(reason) };
        if reason.len() > 123 {
            return Err("WebSocket close reason exceeds 123 bytes".into());
        }
        with_socket(handle, |socket| {
            socket.close(Some(tungstenite::protocol::CloseFrame {
                code,
                reason: reason.to_owned().into(),
            }))
        })
    })();
    string(result.err().unwrap_or_default())
}

unsafe fn invoke(
    callback: Callback,
    handle: i64,
    kind: &str,
    message: &str,
    payload: &[u8],
    state: &str,
) -> Result<(bool, String), String> {
    let _event = Execution::new();
    let bytes = nerv_array_new(payload.len() as i64, 1);
    if !payload.is_empty() {
        let data = unsafe { (*bytes.cast::<TypedArray>()).data.cast::<u8>() };
        unsafe {
            std::ptr::copy_nonoverlapping(payload.as_ptr(), data, payload.len());
        }
    }
    let result = unsafe {
        callback(
            handle,
            string(kind.to_owned()),
            string(message.to_owned()),
            bytes,
            string(state.to_owned()),
        )
    };
    let result = unsafe { result.as_ref() }.ok_or("invalid WebSocket callback result")?;
    if result.length != 2 {
        return Err("WebSocket callback must return [action, state]".into());
    }
    let data = result.data.cast::<*const NervString>();
    let action = unsafe { text(data.read()) };
    let next = unsafe { text(data.add(1).read()) };
    if next.len() > LIMIT {
        return Err("WebSocket callback state exceeds 4 MiB".into());
    }
    match action {
        "continue" => Ok((true, next.to_owned())),
        "close" => Ok((false, next.to_owned())),
        _ => Err("WebSocket callback action must be continue or close".into()),
    }
}

unsafe fn run_socket(
    url: &str,
    headers: &[String],
    initial: &str,
    milliseconds: i64,
    ca_file: &str,
    callback: Callback,
) -> (String, String) {
    let mut state = initial.to_owned();
    let result = (|| -> Result<(), String> {
        if state.len() > LIMIT {
            return Err("WebSocket state exceeds 4 MiB".into());
        }
        let socket = open(url, headers, duration(milliseconds)?, ca_file)?;
        let handle = NEXT_SOCKET
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| "WebSocket handle space exhausted")?;
        SOCKETS.with(|s| {
            s.borrow_mut().insert(handle, socket);
        });
        let _lease = Lease(handle);
        let (keep_going, next) = unsafe { invoke(callback, handle, "open", "", &[], &state) }?;
        state = next;
        if !keep_going {
            let _ = with_socket(handle, |s| s.close(None));
            return Ok(());
        }
        loop {
            let message = SOCKETS.with(|s| s.borrow_mut().get_mut(&handle).unwrap().read());
            let (kind, text, bytes, terminal) = match message {
                Ok(Message::Text(value)) => ("text", value.to_string(), Vec::new(), false),
                Ok(Message::Binary(value)) => ("binary", String::new(), value.to_vec(), false),
                Ok(Message::Ping(value)) => {
                    with_socket(handle, |s| s.flush())?;
                    ("ping", String::new(), value.to_vec(), false)
                }
                Ok(Message::Pong(value)) => ("pong", String::new(), value.to_vec(), false),
                Ok(Message::Close(value)) => {
                    let info = value
                        .map(|c| {
                            serde_json::json!({"code":u16::from(c.code),"reason":c.reason.as_str()})
                                .to_string()
                        })
                        .unwrap_or_else(|| "null".into());
                    let _ = with_socket(handle, |s| s.flush());
                    ("close", info, Vec::new(), true)
                }
                Ok(Message::Frame(_)) => continue,
                Err(tungstenite::Error::Io(e))
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    ("timeout", String::new(), Vec::new(), false)
                }
                Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                    ("close", "null".into(), Vec::new(), true)
                }
                Err(error) => {
                    let error = error.to_string();
                    let (_, next) =
                        unsafe { invoke(callback, handle, "error", &error, &[], &state) }?;
                    state = next;
                    return Err(error);
                }
            };
            let (keep_going, next) =
                unsafe { invoke(callback, handle, kind, &text, &bytes, &state) }?;
            state = next;
            if terminal || !keep_going {
                if !terminal {
                    let _ = with_socket(handle, |s| s.close(None));
                }
                return Ok(());
            }
        }
    })();
    (state, result.err().unwrap_or_default())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_net_websocket_run(
    url: *const NervString,
    headers: *const ArrayHeader,
    state: *const NervString,
    timeout: i64,
    ca_file: *const NervString,
    callback: Callback,
) -> *mut c_void {
    let (state, error) = match unsafe { string_array(headers) } {
        Ok(headers) => unsafe {
            run_socket(
                text(url),
                &headers,
                text(state),
                timeout,
                text(ca_file),
                callback,
            )
        },
        Err(error) => (unsafe { text(state) }.to_owned(), error),
    };
    strings(vec![state, error])
}

thread_local! {
    static CLOSURE_CALLBACKS: RefCell<Vec<*const super::closure::Closure>> = const { RefCell::new(Vec::new()) };
}

struct ClosureCallback;
impl Drop for ClosureCallback {
    fn drop(&mut self) {
        CLOSURE_CALLBACKS.with(|callbacks| {
            callbacks.borrow_mut().pop();
        });
    }
}

unsafe extern "C" fn invoke_closure(
    handle: i64,
    kind: *const NervString,
    text: *const NervString,
    bytes: *const c_void,
    state: *const NervString,
) -> *const ArrayHeader {
    let closure = CLOSURE_CALLBACKS
        .with(|callbacks| *callbacks.borrow().last().expect("active closure callback"));
    let closure = unsafe { &*closure };
    let callback: unsafe extern "C" fn(
        *mut c_void,
        i64,
        *const NervString,
        *const NervString,
        *const c_void,
        *const NervString,
    ) -> *const ArrayHeader = unsafe { std::mem::transmute(closure.code) };
    unsafe { callback(closure.environment, handle, kind, text, bytes, state) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn nerv_net_websocket_run_closure(
    url: *const NervString,
    headers: *const ArrayHeader,
    state: *const NervString,
    timeout: i64,
    ca_file: *const NervString,
    callback: *const super::closure::Closure,
) -> *mut c_void {
    if unsafe { (*callback).mutable_captures } != 0 {
        return strings(vec![unsafe { text(state) }.to_owned(), "WebSocket callbacks cannot capture arrays, aggregates, or functions; use explicit state".into()]);
    }
    CLOSURE_CALLBACKS.with(|callbacks| callbacks.borrow_mut().push(callback));
    let _scope = ClosureCallback;
    unsafe { nerv_net_websocket_run(url, headers, state, timeout, ca_file, invoke_closure) }
}

pub(super) fn symbols() -> Vec<RuntimeSymbol> {
    vec![
        RuntimeSymbol {
            name: "nerv_net_websocket_run_closure",
            address: nerv_net_websocket_run_closure as *const (),
        },
        RuntimeSymbol {
            name: "nerv_net_http_request",
            address: nerv_net_http_request as *const (),
        },
        RuntimeSymbol {
            name: "nerv_net_websocket_run",
            address: nerv_net_websocket_run as *const (),
        },
        RuntimeSymbol {
            name: "nerv_net_websocket_send",
            address: nerv_net_websocket_send as *const (),
        },
        RuntimeSymbol {
            name: "nerv_net_websocket_send_binary",
            address: nerv_net_websocket_send_binary as *const (),
        },
        RuntimeSymbol {
            name: "nerv_net_websocket_close_with_code",
            address: nerv_net_websocket_close_with_code as *const (),
        },
        RuntimeSymbol {
            name: "nerv_net_websocket_close",
            address: nerv_net_websocket_close as *const (),
        },
    ]
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;
    use std::{io::Write, net::TcpListener, thread};

    unsafe extern "C" fn allocation_callback(
        _: i64,
        _: *const NervString,
        _: *const NervString,
        _: *const c_void,
        state: *const NervString,
    ) -> *const ArrayHeader {
        let count = unsafe { text(state) }.parse::<usize>().unwrap() + 1;
        for _ in 0..30 {
            string("temporary callback allocation".to_owned());
        }
        strings(vec![
            if count >= 101 { "close" } else { "continue" }.to_owned(),
            count.to_string(),
        ])
        .cast()
    }

    #[test]
    fn event_regions_reclaim_allocations_and_socket_handles_expire() {
        let _execution = Execution::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("ws://127.0.0.1:{}/", listener.local_addr().unwrap().port());
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            for _ in 0..100 {
                socket.send(Message::Text("event".into())).unwrap();
            }
            assert!(matches!(socket.read().unwrap(), Message::Close(_)));
            let _ = socket.flush();
        });
        let before = super::super::active_region_allocations();
        let (state, error) = unsafe { run_socket(&url, &[], "0", 1000, "", allocation_callback) };
        assert_eq!(error, "");
        assert_eq!(state, "101");
        assert_eq!(super::super::active_region_allocations(), before);
        SOCKETS.with(|s| assert!(s.borrow().is_empty()));
        assert!(with_socket(0, |s| s.flush()).is_err());
        server.join().unwrap();
    }

    unsafe extern "C" fn timeout_callback(
        _: i64,
        kind: *const NervString,
        _: *const NervString,
        _: *const c_void,
        state: *const NervString,
    ) -> *const ArrayHeader {
        let count = unsafe { text(state) }.parse::<usize>().unwrap()
            + usize::from(unsafe { text(kind) } == "timeout");
        strings(vec![
            if count >= 3 { "close" } else { "continue" }.to_owned(),
            count.to_string(),
        ])
        .cast()
    }

    #[test]
    fn idle_connections_deliver_timeouts_for_application_heartbeats() {
        let _execution = Execution::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("ws://127.0.0.1:{}/", listener.local_addr().unwrap().port());
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            assert!(matches!(socket.read().unwrap(), Message::Close(_)));
            let _ = socket.flush();
        });
        let start = Instant::now();
        let (state, error) = unsafe { run_socket(&url, &[], "0", 5, "", timeout_callback) };
        assert_eq!(state, "3");
        assert_eq!(error, "");
        assert!(start.elapsed() >= Duration::from_millis(10));
        server.join().unwrap();
    }

    #[test]
    fn http_timeouts_and_response_limits_are_enforced() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://127.0.0.1:{}/",
            listener.local_addr().unwrap().port()
        );
        let server = thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            thread::sleep(Duration::from_millis(200));
        });
        let start = Instant::now();
        assert!(http_request("GET", &url, &[], "", 30, "").is_err());
        assert!(start.elapsed() < Duration::from_secs(5));
        server.join().unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://127.0.0.1:{}/",
            listener.local_addr().unwrap().port()
        );
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0; 4096];
            let _ = stream.read(&mut buffer);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                LIMIT + 1
            )
            .unwrap();
        });
        assert!(
            http_request("GET", &url, &[], "", 1000, "")
                .unwrap_err()
                .contains("exceeds 4 MiB")
        );
        server.join().unwrap();
    }

    #[test]
    fn invalid_parameters_are_errors() {
        assert!(http_request("GET", "file:///tmp/data", &[], "", 1, "").is_err());
        assert!(
            http_request(
                "GET",
                "https://example.invalid/",
                &["unpaired".into()],
                "",
                1,
                ""
            )
            .is_err()
        );
        assert!(http_request("GET", "https://example.invalid/", &[], "", 0, "").is_err());
        assert!(tls_config("missing-ca-file.pem").is_err());
    }

    unsafe extern "C" fn partial_callback(
        handle: i64,
        kind: *const NervString,
        message: *const NervString,
        _: *const c_void,
        state: *const NervString,
    ) -> *const ArrayHeader {
        let kind = unsafe { text(kind) };
        let state = unsafe { text(state) };
        if kind == "timeout" && state != "sent" {
            let error = unsafe { nerv_net_websocket_send(handle, string("during-frame".into())) };
            assert_eq!(unsafe { text(error) }, "");
            return strings(vec!["continue".into(), "sent".into()]).cast();
        }
        if kind == "text" {
            assert_eq!(unsafe { text(message) }, "hello");
            assert_eq!(state, "sent");
            return strings(vec!["close".into(), "done".into()]).cast();
        }
        strings(vec!["continue".into(), state.to_owned()]).cast()
    }

    #[test]
    fn read_timeouts_preserve_partial_frames_and_allow_writes() {
        let _execution = Execution::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("ws://127.0.0.1:{}/", listener.local_addr().unwrap().port());
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            socket.get_mut().write_all(&[0x81, 5, b'h', b'e']).unwrap();
            assert_eq!(socket.read().unwrap().into_text().unwrap(), "during-frame");
            socket.get_mut().write_all(b"llo").unwrap();
            assert!(matches!(socket.read().unwrap(), Message::Close(_)));
            let _ = socket.flush();
        });
        let (state, error) = unsafe { run_socket(&url, &[], "initial", 5, "", partial_callback) };
        assert_eq!(error, "");
        assert_eq!(state, "done");
        server.join().unwrap();
    }

    unsafe extern "C" fn invalid_callback(
        _: i64,
        _: *const NervString,
        _: *const NervString,
        _: *const c_void,
        _: *const NervString,
    ) -> *const ArrayHeader {
        strings(vec!["invalid action".into(), "state".into()]).cast()
    }

    #[test]
    fn callback_errors_release_connections_and_event_allocations() {
        let _execution = Execution::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("ws://127.0.0.1:{}/", listener.local_addr().unwrap().port());
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            assert!(socket.read().is_err());
        });
        let before = super::super::active_region_allocations();
        let (state, error) =
            unsafe { run_socket(&url, &[], "initial", 1000, "", invalid_callback) };
        assert_eq!(state, "initial");
        assert!(error.contains("callback action"));
        assert_eq!(super::super::active_region_allocations(), before);
        SOCKETS.with(|s| assert!(s.borrow().is_empty()));
        server.join().unwrap();
    }
}
