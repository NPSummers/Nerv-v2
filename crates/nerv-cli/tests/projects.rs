use std::{
    env, fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = env::temp_dir().join(format!(
            "nerv-project-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn cli(&self, directory: &str, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_nerv"))
            .current_dir(self.0.join(directory))
            .args(args)
            .output()
            .unwrap()
    }
    fn ok(&self, directory: &str, args: &[&str]) -> Output {
        let output = self.cli(directory, args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            text(&output.stderr)
        );
        output
    }
    fn write(&self, path: &str, source: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
}

#[test]
fn creates_checks_tests_builds_and_runs_a_project() {
    let workspace = Workspace::new();
    workspace.ok("", &["new", "app"]);
    assert!(workspace.0.join("app/Nerv.lock").is_file());
    workspace.ok("app", &["check", "--locked"]);
    workspace.ok("app/src", &["check", "--locked"]);
    workspace.ok("app", &["test", "--locked"]);
    workspace.ok("app", &["fmt", "--check"]);
    let output = workspace.ok("app", &["run", "--locked"]);
    assert_eq!(text(&output.stdout), "Hello from Nerv\n");
    workspace.ok("app", &["build", "--release", "--locked"]);
    assert!(!workspace.cli("", &["new", "app"]).status.success());
}

#[test]
fn forwards_arguments_and_exit_status_without_printing_main_result() {
    let workspace = Workspace::new();
    workspace.ok("", &["new", "app"]);
    workspace.write("app/src/main.nerv", "import nerv.std\nfn main() -> i64\n    let args = arguments()\n    println(args[1])\n    println(args[2])\n    37\n");
    let output = workspace.cli("app", &["run", "--", "two words", "--flag"]);
    assert_eq!(output.status.code(), Some(37), "{}", text(&output.stderr));
    assert_eq!(text(&output.stdout), "two words\n--flag\n");
    let help_argument = workspace.cli("app", &["run", "--", "--help", "value"]);
    assert_eq!(help_argument.status.code(), Some(37));
    assert_eq!(text(&help_argument.stdout), "--help\nvalue\n");
    workspace.write("app/src/main.nerv", "fn main() -> i64 => -1\n");
    assert_eq!(workspace.cli("app", &["run"]).status.code(), Some(255));
    workspace.write(
        "app/src/main.nerv",
        "fn main() -> void\n    println(\"done\")\n",
    );
    assert_eq!(text(&workspace.ok("app", &["run"]).stdout), "done\n");
}

#[test]
fn resolves_local_dependencies_and_enforces_portable_lockfiles() {
    let workspace = Workspace::new();
    workspace.ok("", &["new", "app"]);
    workspace.ok("", &["new", "--lib", "math"]);
    workspace.write("app/Nerv.toml", "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\nmath = { path = \"../math\" }\n");
    workspace.write(
        "app/src/main.nerv",
        "import math\nfn main() -> i64\n    println(answer())\n    0\n",
    );
    assert!(
        !workspace
            .cli("app", &["check", "--locked"])
            .status
            .success()
    );
    workspace.ok("app", &["check"]);
    let lock = fs::read_to_string(workspace.0.join("app/Nerv.lock")).unwrap();
    assert!(lock.contains("../math/Nerv.toml"));
    assert!(!lock.contains(&workspace.0.to_string_lossy().to_string()));
    assert_eq!(
        text(&workspace.ok("app", &["run", "--locked"]).stdout),
        "42\n"
    );
    workspace.write(
        "app/tests/dependency.nerv",
        "import math\ntest \"dependency\"\n    assert(answer() == 42)\n",
    );
    workspace.ok("app", &["test", "--locked"]);
    fs::write(workspace.0.join("app/Nerv.lock"), "tampered").unwrap();
    assert!(
        !workspace
            .cli("app", &["check", "--locked"])
            .status
            .success()
    );
    workspace.ok("app", &["check"]);
    assert_eq!(
        fs::read_to_string(workspace.0.join("app/Nerv.lock")).unwrap(),
        lock
    );
}

#[test]
fn invalidates_cache_for_content_options_dependency_and_output_changes() {
    let workspace = Workspace::new();
    workspace.ok("", &["new", "app"]);
    workspace.ok("", &["new", "--lib", "math"]);
    workspace.write("app/Nerv.toml", "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\nmath = { path = \"../math\" }\n");
    workspace.write(
        "app/src/main.nerv",
        "import math\nfn main() -> i64\n    println(answer())\n    0\n",
    );
    workspace.ok("app", &["build"]);
    assert!(text(&workspace.ok("app", &["build"]).stderr).contains("Fresh"));
    workspace.write("math/src/lib.nerv", "fn answer() -> i64 => 43\n");
    let rebuilt = workspace.ok("app", &["run"]);
    assert!(!text(&rebuilt.stderr).contains("Fresh"));
    assert_eq!(text(&rebuilt.stdout), "43\n");
    assert!(!text(&workspace.ok("app", &["build", "--cpu", "native"]).stderr).contains("Fresh"));
    assert!(!text(&workspace.ok("app", &["build", "--no-cache"]).stderr).contains("Fresh"));
    let executable = workspace.0.join(if cfg!(windows) {
        "app/target/nerv/debug/app.exe"
    } else {
        "app/target/nerv/debug/app"
    });
    fs::write(&executable, b"tampered").unwrap();
    workspace.ok("app", &["build"]);
    assert_eq!(text(&workspace.ok("app", &["run"]).stdout), "43\n");
    assert!(
        !workspace
            .cli("app", &["build", "-o", "Nerv.toml"])
            .status
            .success()
    );
    assert!(
        !workspace
            .cli("app", &["build", "-o", "Nerv.lock"])
            .status
            .success()
    );
}

#[test]
fn reports_cycles_and_rejects_remote_dependencies() {
    let workspace = Workspace::new();
    workspace.ok("", &["new", "--lib", "one"]);
    workspace.ok("", &["new", "--lib", "two"]);
    for (name, dependency) in [("one", "two"), ("two", "one")] {
        workspace.write(&format!("{name}/Nerv.toml"), &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n[dependencies]\n{dependency} = {{ path = \"../{dependency}\" }}\n"));
    }
    let cycle = workspace.cli("one", &["check"]);
    assert!(!cycle.status.success());
    assert!(text(&cycle.stderr).contains("dependency cycle"));
    workspace.write(
        "one/Nerv.toml",
        "[package]\nname = \"one\"\nversion = \"0.1.0\"\n[dependencies]\nother = \"1.0\"\n",
    );
    assert!(text(&workspace.cli("one", &["check"]).stderr).contains("only local dependencies"));
}

#[test]
fn tests_continue_after_an_assertion_failure() {
    let workspace = Workspace::new();
    workspace.ok("", &["new", "app"]);
    workspace.write(
        "app/tests/a_fail.nerv",
        "test \"fails\"\n    assert(false)\n",
    );
    workspace.write(
        "app/tests/b_pass.nerv",
        "test \"passes\"\n    println(\"still running\")\n",
    );
    let output = workspace.cli("app", &["test"]);
    assert!(!output.status.success());
    assert!(text(&output.stdout).contains("still running"));
    assert!(text(&output.stdout).contains("1 failed"));
}

#[test]
fn formatter_is_idempotent_and_does_not_write_invalid_sources() {
    let workspace = Workspace::new();
    workspace.ok("", &["new", "app"]);
    workspace.write("app/src/main.nerv", "fn main( )->i64\n  1+2\n");
    assert!(!workspace.cli("app", &["fmt", "--check"]).status.success());
    workspace.ok("app", &["fmt"]);
    workspace.ok("app", &["fmt", "--check"]);
    assert_eq!(
        fs::read_to_string(workspace.0.join("app/src/main.nerv")).unwrap(),
        "fn main() -> i64\n    1 + 2\n"
    );
    workspace.write("app/src/bad.nerv", "fn !!!\n");
    assert!(!workspace.cli("app", &["fmt"]).status.success());
    assert_eq!(
        fs::read_to_string(workspace.0.join("app/src/bad.nerv")).unwrap(),
        "fn !!!\n"
    );
}

#[test]
fn standalone_modern_and_legacy_entry_contracts_are_both_available() {
    let workspace = Workspace::new();
    workspace.write("main.nerv", "fn main() -> i64 => 7\n");
    assert_eq!(
        workspace.cli("", &["run", "main.nerv"]).status.code(),
        Some(7)
    );
    assert_eq!(workspace.cli("", &["main.nerv"]).status.code(), Some(7));
    assert_eq!(
        text(&workspace.ok("", &["main.nerv", "--run"]).stdout),
        "7\n"
    );
    workspace.ok("", &["check", "main.nerv"]);
}

#[test]
fn resolves_transitive_dependencies_in_their_own_package_context() {
    let workspace = Workspace::new();
    workspace.ok("", &["new", "app"]);
    workspace.ok("", &["new", "--lib", "outer"]);
    workspace.ok("", &["new", "--lib", "inner"]);
    workspace.write("outer/Nerv.toml", "[package]\nname = \"outer\"\nversion = \"0.1.0\"\n[dependencies]\nleaf = { path = \"../inner\" }\n");
    workspace.write(
        "outer/src/lib.nerv",
        "import leaf\nfn wrapped() -> i64 => answer()\n",
    );
    workspace.write("app/Nerv.toml", "[package]\nname = \"app\"\nversion = \"0.1.0\"\n[dependencies]\nouter = { path = \"../outer\" }\n");
    workspace.write(
        "app/src/main.nerv",
        "import outer\nfn main() -> i64\n    println(wrapped())\n    0\n",
    );
    assert_eq!(text(&workspace.ok("app", &["run"]).stdout), "42\n");
}

#[test]
fn each_source_files_tests_run_once() {
    let workspace = Workspace::new();
    workspace.ok("", &["new", "app"]);
    workspace.write(
        "app/src/helper.nerv",
        "fn helper() -> i64 => 42\ntest \"helper\"\n    println(\"helper marker\")\n",
    );
    workspace.write(
        "app/src/main.nerv",
        "import helper\nfn main() -> i64 => 0\ntest \"main\"\n    assert(helper() == 42)\n",
    );
    let output = workspace.ok("app", &["test"]);
    assert_eq!(text(&output.stdout).matches("helper marker").count(), 1);
}

#[test]
fn legacy_file_build_behavior_does_not_depend_on_option_order() {
    let workspace = Workspace::new();
    workspace.write("main.nerv", "fn main() -> i64 => 7\n");
    workspace.ok(
        "",
        &[
            "build",
            "--release",
            "main.nerv",
            "-o",
            if cfg!(windows) { "app.exe" } else { "app" },
        ],
    );
    let output = Command::new(
        workspace
            .0
            .join(if cfg!(windows) { "app.exe" } else { "app" }),
    )
    .output()
    .unwrap();
    assert!(output.status.success());
    assert_eq!(text(&output.stdout), "7\n");
}
