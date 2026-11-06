use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Project(PathBuf);
impl Project {
    fn new() -> Self {
        let path = env::temp_dir().join(format!(
            "nerv-native-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn executable(&self) -> PathBuf {
        self.0.join(if cfg!(windows) { "app.exe" } else { "app" })
    }
    fn cli(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_nerv"));
        command.current_dir(&self.0);
        command
    }
    fn build(&self, source: &Path, mode: &str) -> Output {
        self.cli()
            .arg("build")
            .arg(source)
            .arg("-o")
            .arg(self.executable())
            .arg(mode)
            .output()
            .unwrap()
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .replace("\r\n", "\n")
}

fn clean_command(executable: &Path, directory: &Path) -> Command {
    let mut command = Command::new(executable);
    command.current_dir(directory).env("PATH", "");
    for key in [
        "LLVM_SYS_231_PREFIX",
        "LLVM_CONFIG_PATH",
        "DYLD_LIBRARY_PATH",
        "DYLD_FALLBACK_LIBRARY_PATH",
        "LD_LIBRARY_PATH",
        "CARGO_HOME",
        "RUSTUP_HOME",
    ] {
        command.env_remove(key);
    }
    command
}

fn parity(source: &Path) {
    let project = Project::new();
    let jit = project.cli().arg(source).arg("--run").output().unwrap();
    assert!(
        matches!(jit.status.code(), Some(0 | 1)),
        "{}: {jit:?}",
        source.display()
    );
    for mode in ["--debug", "--release"] {
        let build = project.build(source, mode);
        assert!(
            build.status.success(),
            "{} {mode}: {}",
            source.display(),
            text(&build.stderr)
        );
        let native = clean_command(&project.executable(), &project.0)
            .output()
            .unwrap();
        assert_eq!(
            native.status.code(),
            jit.status.code(),
            "{} {mode}: {}",
            source.display(),
            text(&native.stderr)
        );
        assert_eq!(
            text(&native.stdout),
            text(&jit.stdout),
            "{} {mode}",
            source.display()
        );
        assert_eq!(
            text(&native.stderr),
            text(&jit.stderr),
            "{} {mode}",
            source.display()
        );
        assert!(!fs::read_dir(&project.0).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".nerv-build-")
        }));
    }
}

#[test]
fn native_cpu_and_invalid_cpu_options() {
    let project = Project::new();
    let source = project.0.join("main.nerv");
    fs::write(&source, "fn main() -> i64 => 42\n").unwrap();
    let output = project
        .cli()
        .arg("build")
        .arg(&source)
        .arg("-o")
        .arg(project.executable())
        .args(["--release", "--cpu=native"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output.stderr));
    let result = clean_command(&project.executable(), &project.0)
        .output()
        .unwrap();
    assert!(result.status.success());
    assert_eq!(text(&result.stdout), "42\n");
    for options in [
        vec!["--cpu", "bad"],
        vec!["--cpu"],
        vec!["--cpu=native", "--cpu=generic"],
    ] {
        let output = project
            .cli()
            .arg("build")
            .arg(&source)
            .args(options)
            .output()
            .unwrap();
        assert!(!output.status.success());
    }
}

#[test]
fn inline_bounds_checks_reject_negative_huge_and_empty_indices() {
    let project = Project::new();
    let source = project.0.join("main.nerv");
    for (array, index) in [
        ("[1]", "-1"),
        ("[1]", "9223372036854775807"),
        ("nerv.arrays.new(0, 0)", "0"),
    ] {
        fs::write(
            &source,
            format!(
                "fn main() -> i64\n    let values: []i64 = {array}\n    return values[{index}]\n"
            ),
        )
        .unwrap();
        parity(&source);
    }
}

#[test]
fn reassigned_handles_and_wrapping_indices_retain_bounds_checks() {
    let project = Project::new();
    let source = project.0.join("main.nerv");
    for body in [
        "    let mut values: []i64 = [1, 2]\n    for i in 0..2\n        println(values[1])\n        values = [1]\n",
        "    let values: []i64 = [1, 2]\n    let index = 9223372036854775807 + 1\n    println(values[index])\n",
        "    let values: []i64 = [1, 2]\n    for i in 0..=2\n        println(values[i])\n",
    ] {
        fs::write(&source, format!("fn main() -> void\n{body}")).unwrap();
        let jit = project.cli().arg(&source).arg("--run").output().unwrap();
        assert_eq!(jit.status.code(), Some(1));
        assert!(text(&jit.stderr).contains("array index out of bounds"));
        parity(&source);
    }
}

macro_rules! fixtures {
    ($($name:ident => $fixture:literal),+ $(,)?) => { $(
        #[test]
        fn $name() {
            parity(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join($fixture).join("main.nerv"));
        }
    )+ };
}
fixtures! {
    structs => "run/structs",
    enums => "run/enums",
    traits_methods => "run/traits_methods",
    arrays => "run/arrays",
    loop_optimizations => "run/loop_optimizations",
    lambdas => "run/lambdas",
    imports => "run/imports",
    output_and_exit_status => "run/output",
    resolved_dispatch => "run/resolved_dispatch",
    indirect_calls => "run/indirect_calls",
    field_order_and_conversions => "run/field_order",
    nested_shared_returns => "run/region_lifetimes",
    bounds_failure => "fail/bounds",
    division_failure => "fail/division",
    assertion_failure => "fail/assertion",
    array_allocation_failure => "fail/array_allocation",
    string_allocation_failure => "fail/string_allocation",
}

#[test]
fn executable_specification_matches_jit_in_both_modes() {
    let project = Project::new();
    let path = project.0.join("main.nerv");
    let mut lines = include_str!("../../../docs/language-spec.md").lines();
    let mut cases = 0;
    while let Some(line) = lines.next() {
        let Some(expectation) = line
            .strip_prefix("Example (`")
            .and_then(|s| s.strip_suffix("`):"))
        else {
            continue;
        };
        assert_eq!(lines.next(), Some(""));
        assert_eq!(lines.next(), Some("```nerv"));
        let mut source = String::new();
        for line in lines.by_ref() {
            if line == "```" {
                break;
            }
            source.push_str(line);
            source.push('\n');
        }
        if expectation.starts_with("run:") || expectation.starts_with("fail:") {
            fs::write(&path, source).unwrap();
            parity(&path);
            cases += 1;
        }
    }
    assert!(cases >= 30, "missing native specification coverage");
}

#[test]
fn builds_from_unicode_paths_and_emits_a_real_host_object() {
    let project = Project::new();
    let source = project.0.join("hello world é.nerv");
    fs::write(&source, "fn main() -> i64 => 42\n").unwrap();
    let object = project.0.join("object with spaces.o");
    let output = project
        .cli()
        .arg("build")
        .arg(&source)
        .arg("--emit-object")
        .arg("-o")
        .arg(&object)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output.stderr));
    let bytes = fs::read(object).unwrap();
    assert!(
        bytes
            .windows(b"nerv_runtime_abi_v2".len())
            .any(|window| window == b"nerv_runtime_abi_v2")
    );
    assert!(bytes.len() > 64);
    if cfg!(target_os = "macos") {
        assert_eq!(&bytes[..4], &[0xcf, 0xfa, 0xed, 0xfe]);
    }
    if cfg!(target_os = "linux") {
        assert_eq!(&bytes[..4], b"\x7fELF");
    }
    let output = project.cli().arg("build").arg(&source).output().unwrap();
    assert!(output.status.success(), "{}", text(&output.stderr));
    let default_output = project.0.join(if cfg!(windows) {
        "hello world é.exe"
    } else {
        "hello world é"
    });
    let result = clean_command(&default_output, &project.0).output().unwrap();
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(text(&result.stdout), "42\n");
}

#[test]
fn failed_builds_preserve_source_and_existing_output_and_clean_up() {
    let project = Project::new();
    let source = project.0.join("main.nerv");
    let contents = "fn main() -> i64 => 42\n";
    fs::write(&source, contents).unwrap();
    fs::write(project.executable(), "existing output").unwrap();
    let failure = project
        .cli()
        .arg("build")
        .arg(&source)
        .arg("-o")
        .arg(project.executable())
        .env("NERV_LINKER", project.0.join("missing-linker"))
        .output()
        .unwrap();
    assert_eq!(failure.status.code(), Some(1));
    assert!(text(&failure.stderr).contains("could not run native linker"));
    assert_eq!(
        fs::read_to_string(project.executable()).unwrap(),
        "existing output"
    );
    let failure = project
        .cli()
        .arg("build")
        .arg(&source)
        .arg("-o")
        .arg(&source)
        .output()
        .unwrap();
    assert_eq!(failure.status.code(), Some(1));
    assert!(text(&failure.stderr).contains("overwrite a source file"));
    assert_eq!(fs::read_to_string(&source).unwrap(), contents);
    assert!(!fs::read_dir(&project.0).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".nerv-build-")
    }));
}

#[test]
fn reports_invalid_build_options_and_missing_entry() {
    let project = Project::new();
    let source = project.0.join("library.nerv");
    fs::write(&source, "fn answer() -> i64 => 42\n").unwrap();
    for (args, message) in [
        (vec!["build"], "usage:"),
        (vec!["build", "-o"], "requires an output path"),
        (vec!["build", "--bad-option"], "unknown build option"),
        (vec!["build", "--debug", "--release"], "choose only one"),
    ] {
        let result = project.cli().args(args).output().unwrap();
        assert_eq!(result.status.code(), Some(1));
        assert!(text(&result.stderr).contains(message), "{result:?}");
    }
    let result = project.build(&source, "--release");
    assert_eq!(result.status.code(), Some(1));
    assert!(text(&result.stderr).contains("missing `main`"));
    assert!(!project.executable().exists());
}

#[test]
fn standalone_program_has_no_llvm_dynamic_dependency() {
    let project = Project::new();
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/run/region_lifetimes/main.nerv");
    let built = project.build(&source, "--release");
    assert!(built.status.success(), "{}", text(&built.stderr));
    #[cfg(target_os = "macos")]
    let output = Command::new("otool")
        .arg("-L")
        .arg(project.executable())
        .output()
        .unwrap();
    #[cfg(target_os = "linux")]
    let output = Command::new("ldd")
        .arg(project.executable())
        .output()
        .unwrap();
    #[cfg(windows)]
    let output = find_msvc_tools::find(env!("NERV_BUILD_TARGET"), "dumpbin.exe")
        .unwrap()
        .arg("/DEPENDENTS")
        .arg(project.executable())
        .output()
        .unwrap();
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    {
        assert!(output.status.success(), "{output:?}");
        let dependencies = text(&output.stdout).to_lowercase();
        assert!(
            !dependencies.contains("libllvm") && !dependencies.contains("llvm-c.dll"),
            "{dependencies}"
        );
    }
    let result = clean_command(&project.executable(), &project.0)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(0));
    assert_eq!(text(&result.stdout), "42\n");
}

#[test]
fn scalar_io_runtime_builtins_and_void_entry_match() {
    let project = Project::new();
    let source = project.0.join("main.nerv");
    fs::write(&source, "fn main() -> void\n    println(true)\n    println(1.5)\n    println(nerv.math.sqrt(9.0))\n    println(nerv.string.to_upper(\"é\"))\n    println(nerv.string.replace(\"a-b\", \"-\", \"/\"))\n    println(nerv.string.split(\"a:b\", \":\"))\n").unwrap();
    parity(&source);
}

#[test]
fn debug_loop_locals_do_not_grow_the_stack_per_iteration() {
    let project = Project::new();
    let source = project.0.join("main.nerv");
    fs::write(&source, "fn main() -> i64\n    let mut total = 0\n    for index in 0..2000000\n        let copy = index\n        total = total + copy\n    total\n").unwrap();
    parity(&source);
}

#[test]
fn output_before_a_fatal_runtime_error_is_preserved() {
    let project = Project::new();
    let source = project.0.join("main.nerv");
    fs::write(
        &source,
        "fn main() -> i64\n    print(\"before\")\n    assert(false)\n    0\n",
    )
    .unwrap();
    parity(&source);
}

#[test]
fn reciprocal_setup_preserves_output_before_zero_division() {
    let project = Project::new();
    let source = project.0.join("main.nerv");
    fs::write(&source, "fn main() -> i64\n    let zero = nerv.random(1)\n    for i in 0..64\n        print(\"before\")\n        println(i / zero)\n    return 0\n").unwrap();
    let jit = project.cli().arg(&source).arg("--run").output().unwrap();
    assert_eq!(jit.status.code(), Some(1));
    assert_eq!(text(&jit.stdout), "before");
    assert!(text(&jit.stderr).contains("integer division by zero"));
    parity(&source);
}

#[test]
fn compound_enum_payloads_match_jit_in_both_modes() {
    parity(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../example/enum-payloads.nerv"));
}

#[test]
fn generic_specializations_match_jit_in_both_modes() {
    parity(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../example/generics.nerv"));
}

#[test]
fn numeric_types_match_jit_in_both_modes() {
    parity(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../example/numeric-types.nerv"));
}

#[test]
fn standard_library_matches_jit_in_both_modes() {
    parity(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../example/standard-library.nerv"));
}

#[test]
fn standard_library_io_round_trip_and_errors() {
    let project = Project::new();
    let source = project.0.join("main.nerv");
    fs::write(&source, r#"import nerv.std
fn main() -> i64
    let written = write_text("data.txt", "hello UTF-8: ?")
    assert(result_or(written, -1) > 0)
    assert_string_equal(result_or(read_text("data.txt"), "failed"), "hello UTF-8: ?")
    let missing = read_text("missing-file")
    let failed = match missing
        Ok(value) => false
        Err(error) => string_equal(error.operation, "read") and string_equal(error.path, "missing-file") and nerv.string.len(error.message) > 0
    assert(failed)
    assert(result_or(create_directory("subdir"), -1) == 0)
    let entries = result_or(list_directory("subdir"), [""] * 0)
    assert(nerv.arrays.len(entries) == 0)
    let input = result_or(read_stdin(), "failed")
    assert_string_equal(input, "input text")
    assert_string_equal(option_or(environment("NERV_TEST_VALUE"), "missing"), "set")
    assert_string_equal(option_or(environment("NERV_TEST_MISSING"), "missing"), "missing")
    assert_equal(u8(255) + u8(1), u8(0))
    sleep(duration_ms(0))
    stderr("diagnostic")
    println(u64(-1))
    42
"#).unwrap();
    for mode in ["jit", "--debug", "--release"] {
        let mut command = if mode == "jit" {
            let mut command = project.cli();
            command.arg(&source).arg("--run");
            command
        } else {
            let build = project.build(&source, mode);
            assert!(build.status.success(), "{}", text(&build.stderr));
            clean_command(&project.executable(), &project.0)
        };
        command
            .env("NERV_TEST_VALUE", "set")
            .env_remove("NERV_TEST_MISSING")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        let mut child = command.spawn().unwrap();
        std::io::Write::write_all(&mut child.stdin.take().unwrap(), b"input text").unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{mode}: {}", text(&output.stderr));
        assert_eq!(text(&output.stdout), "18446744073709551615\n42\n");
        assert_eq!(text(&output.stderr), "diagnostic");
    }
}

#[test]
fn file_processing_example_uses_program_arguments() {
    let project = Project::new();
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../example/file-processing.nerv");
    let build = project.build(&source, "--release");
    assert!(build.status.success(), "{}", text(&build.stderr));
    fs::write(project.0.join("input.txt"), "  hello UTF-8: ?  ").unwrap();
    let output = clean_command(&project.executable(), &project.0)
        .args(["input.txt", "output.txt"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "0\n");
    assert_eq!(
        fs::read_to_string(project.0.join("output.txt")).unwrap(),
        "hello UTF-8: ?"
    );
}
