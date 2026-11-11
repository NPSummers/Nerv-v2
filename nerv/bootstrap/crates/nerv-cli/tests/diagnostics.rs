use std::{fs, path::Path, process::Command};

fn snapshot(name: &str, mode: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/diagnostics")
        .canonicalize()
        .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_nerv"))
        .current_dir(&root)
        .args([&format!("{name}.nerv"), mode])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    // Only source-root paths and platform line endings differ across hosts.
    let actual = String::from_utf8(output.stderr)
        .unwrap()
        .replace("\r\n", "\n")
        .replace('\\', "/")
        .replace(
            &format!("{}/", root.to_string_lossy().replace('\\', "/")),
            "",
        );
    let expected = fs::read_to_string(root.join(format!("{name}.stderr")))
        .unwrap()
        .replace("\r\n", "\n");
    assert_eq!(actual, expected, "diagnostic snapshot: {name}");
}

macro_rules! snapshots {
    ($($name:ident => $mode:literal),+ $(,)?) => { $(
        #[test]
        fn $name() { snapshot(stringify!($name), $mode); }
    )+ };
}

snapshots! {
    lexer => "--lex",
    parser => "--parse",
    type_mismatch => "--check",
    unknown_name => "--check",
    unknown_function => "--check",
    argument_type => "--check",
    argument_count => "--check",
    independent_errors => "--check",
    imported_error => "--check",
    eof => "--parse",
    backend => "--run",
}

#[test]
fn payload_limit_is_checked_before_codegen() {
    snapshot("backend", "--check");
}
