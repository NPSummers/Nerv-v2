use rustls::{ServerConfig, ServerConnection, StreamOwned, pki_types::PrivatePkcs8KeyDer};
use std::{
    env, fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::{Command, Output},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let path = env::temp_dir().join(format!(
            "nerv-network-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn cli(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_nerv"));
        command.current_dir(&self.0);
        command
    }
    fn program(&self, mode: &str, source: &str) -> Command {
        let path = self.0.join("main.nerv");
        fs::write(&path, source).unwrap();
        if mode == "jit" {
            let mut command = self.cli();
            command.arg(path).arg("--run");
            return command;
        }
        let executable = self.0.join(if cfg!(windows) { "app.exe" } else { "app" });
        let build = self
            .cli()
            .arg("build")
            .arg(path)
            .arg("-o")
            .arg(&executable)
            .arg(mode)
            .output()
            .unwrap();
        assert!(build.status.success(), "{mode}: {}", text(&build.stderr));
        let mut command = Command::new(executable);
        command.current_dir(&self.0);
        for key in [
            "LLVM_SYS_231_PREFIX",
            "LLVM_CONFIG_PATH",
            "DYLD_LIBRARY_PATH",
            "LD_LIBRARY_PATH",
            "DYLD_FALLBACK_LIBRARY_PATH",
            "CARGO_HOME",
            "RUSTUP_HOME",
        ] {
            command.env_remove(key);
        }
        command.env("PATH", "");
        command
    }
    fn tls(&self) -> Arc<ServerConfig> {
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into(), "127.0.0.1".into()])
            .unwrap();
        fs::write(self.0.join("ca.pem"), cert.cert.pem()).unwrap();
        Arc::new(
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(
                    vec![cert.cert.der().clone()],
                    PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der()).into(),
                )
                .unwrap(),
        )
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
}
fn success(output: Output, mode: &str) {
    assert!(output.status.success(), "{mode}: {}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "42\n", "{mode}");
}
fn accept(listener: &TcpListener) -> TcpStream {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                return stream;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => {
                thread::sleep(Duration::from_millis(10))
            }
            Err(e) => panic!("accept: {e}"),
        }
    }
}

#[test]
fn json_round_trips_in_jit_and_both_native_modes() {
    let project = Project::new();
    for mode in ["jit", "--debug", "--release"] {
        success(
            project
                .program(mode, include_str!("../../../example/json.nerv"))
                .output()
                .unwrap(),
            mode,
        );
    }
}

#[test]
fn https_requests_preserve_status_headers_and_json_in_all_modes() {
    let project = Project::new();
    let config = project.tls();
    for mode in ["jit", "--debug", "--release"] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "https://127.0.0.1:{}/test",
            listener.local_addr().unwrap().port()
        );
        let config = config.clone();
        let server = thread::spawn(move || {
            let mut stream =
                StreamOwned::new(ServerConnection::new(config).unwrap(), accept(&listener));
            let mut request = Vec::new();
            let mut buffer = [0; 2048];
            loop {
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buffer[..count]);
                let text = String::from_utf8_lossy(&request);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|n| n.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
                assert!(request.len() < 8192);
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.starts_with("POST /test HTTP/1.1"));
            assert!(request.to_ascii_lowercase().contains("x-test: yes"));
            assert!(request.ends_with("{\"message\":\"hello\"}"));
            let body = "{\"retry_after\":0.05,\"global\":false}";
            write!(stream, "HTTP/1.1 429 Too Many Requests\r\nContent-Type: application/json\r\nRetry-After: 1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            stream.flush().unwrap();
        });
        let mut command = project.program(mode, r#"import nerv.net
fn main() -> i64
    let url = option_or(environment("TEST_URL"), "")
    let ca = option_or(environment("TEST_CA"), "")
    let fallback = HttpResponse { status: 0, headers: json_array(), body: "" }
    let response = result_or(http_request_with_ca("POST", url, ["X-Test", "yes"], "{\"message\":\"hello\"}", 5000, ca), fallback)
    assert(response.status == 429)
    assert_string_equal(option_or(http_header(response, "retry-after"), ""), "1")
    let body = result_or(json_parse(response.body), json_null())
    assert(json_f64(result_or(json_get(body, "retry_after"), json_null()), 0.0) == 0.05)
    assert(not json_bool(result_or(json_get(body, "global"), json_null()), true))
    42
"#);
        command
            .env("TEST_URL", url)
            .env("TEST_CA", project.0.join("ca.pem"));
        success(command.output().unwrap(), mode);
        server.join().unwrap();
    }
}

#[test]
fn wss_callbacks_exchange_text_binary_ping_and_custom_close_in_all_modes() {
    let project = Project::new();
    let config = project.tls();
    for mode in ["jit", "--debug", "--release"] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "wss://127.0.0.1:{}/gateway",
            listener.local_addr().unwrap().port()
        );
        let config = config.clone();
        let server = thread::spawn(move || {
            let stream =
                StreamOwned::new(ServerConnection::new(config).unwrap(), accept(&listener));
            let mut socket = tungstenite::accept_hdr(
                stream,
                |request: &tungstenite::handshake::server::Request,
                 response: tungstenite::handshake::server::Response| {
                    assert_eq!(request.headers()["x-test"], "yes");
                    Ok(response)
                },
            )
            .unwrap();
            assert_eq!(socket.read().unwrap().into_text().unwrap(), "hello");
            socket
                .send(tungstenite::Message::Ping(vec![1].into()))
                .unwrap();
            socket
                .send(tungstenite::Message::Text("{\"n\":42}".into()))
                .unwrap();
            socket
                .send(tungstenite::Message::Binary(vec![0, 255, 1].into()))
                .unwrap();
            let mut replied = false;
            let mut pong = false;
            let mut binary = false;
            loop {
                match socket.read().unwrap() {
                    tungstenite::Message::Text(value) => {
                        assert_eq!(value, "reply");
                        replied = true;
                    }
                    tungstenite::Message::Binary(value) => {
                        assert_eq!(value.as_ref(), &[1, 255]);
                        binary = true;
                    }
                    tungstenite::Message::Pong(value) => {
                        assert_eq!(value.as_ref(), &[1]);
                        pong = true;
                    }
                    tungstenite::Message::Close(close) => {
                        assert_eq!(u16::from(close.unwrap().code), 4000);
                        let _ = socket.flush();
                        break;
                    }
                    other => panic!("unexpected message {other:?}"),
                }
            }
            assert!(replied && pong && binary);
        });
        let mut command = project.program(mode, r#"import nerv.net
fn on_event(handle: i64, kind: string, message: string, bytes: []u8, state: string) -> []string
    if string_equal(kind, "open")
        assert(result_or(websocket_send(handle, "hello"), false))
        ["continue", "opened"]
    else
        if string_equal(kind, "text")
            let event = result_or(json_parse(message), json_null())
            assert(json_i64(result_or(json_get(event, "n"), json_null()), 0) == 42)
            assert(result_or(websocket_send(handle, "reply"), false))
            ["continue", "received"]
        else
            if string_equal(kind, "binary")
                assert(nerv.arrays.len(bytes) == 3)
                assert(bytes[0] == u8(0) and bytes[1] == u8(255) and bytes[2] == u8(1))
                assert(result_or(websocket_send_binary(handle, [u8(1), u8(255)]), false))
                assert(result_or(websocket_close_with_code(handle, 4000, "reconnect"), false))
                ["close", "done"]
            else
                ["continue", state]
fn main() -> i64
    let outcome = websocket_run_with_options(option_or(environment("TEST_URL"), ""), ["X-Test", "yes"], "initial", 1000, option_or(environment("TEST_CA"), ""), on_event)
    assert_string_equal(outcome.error, "")
    assert_string_equal(outcome.state, "done")
    42
"#);
        command
            .env("TEST_URL", url)
            .env("TEST_CA", project.0.join("ca.pem"));
        success(command.output().unwrap(), mode);
        server.join().unwrap();
    }
}

#[test]
fn rejects_untrusted_tls_and_reports_safe_errors_in_all_modes() {
    let project = Project::new();
    let config = project.tls();
    for mode in ["jit", "--debug", "--release"] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "https://127.0.0.1:{}/",
            listener.local_addr().unwrap().port()
        );
        let config = config.clone();
        let server = thread::spawn(move || {
            let mut stream =
                StreamOwned::new(ServerConnection::new(config).unwrap(), accept(&listener));
            let mut byte = [0];
            assert!(stream.read(&mut byte).is_err());
        });
        let mut command = project.program(
            mode,
            r#"import nerv.net
fn main() -> i64
    let response = http_request("GET", option_or(environment("TEST_URL"), ""), [""] * 0, "", 3000)
    let rejected = match response
        Ok(value) => false
        Err(error) => nerv.string.len(error.message) > 0
    assert(rejected)
    assert(not result_or(websocket_send(0, "test"), false))
    assert(not result_or(websocket_close_with_code(0, 1006, "invalid"), false))
    42
"#,
        );
        command.env("TEST_URL", url);
        success(command.output().unwrap(), mode);
        server.join().unwrap();
    }
}

#[test]
fn rejects_incorrect_network_callback_types_before_execution() {
    let project = Project::new();
    let source = project.0.join("bad.nerv");
    fs::write(&source, r#"import nerv.net
fn callback(handle: i64, kind: string, message: string, bytes: []i64, state: string) -> []string => ["close", state]
fn main() -> i64
    websocket_run("ws://127.0.0.1:1", "", 10, callback)
    0
"#).unwrap();
    let output = project.cli().arg(source).arg("--check").output().unwrap();
    assert!(!output.status.success());
    assert!(
        text(&output.stderr).contains("argument type mismatch"),
        "{}",
        text(&output.stderr)
    );
}
