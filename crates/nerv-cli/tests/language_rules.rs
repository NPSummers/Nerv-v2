use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_PROJECT: AtomicU64 = AtomicU64::new(0);

struct Project(PathBuf);

impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "nerv-rules-{}-{}",
            std::process::id(),
            NEXT_PROJECT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn write(&self, path: &str, source: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }

    fn run(&self, mode: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_nerv"))
            .current_dir(&self.0)
            .args(["main.nerv", mode])
            .output()
            .unwrap()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn verify(project: &Project, expectation: &str, source: &str) {
    project.write("main.nerv", source);
    let checked = project.run("--check");
    let diagnostics = String::from_utf8_lossy(&checked.stderr);
    if let Some(message) = expectation.strip_prefix("reject:") {
        assert!(
            !checked.status.success(),
            "unexpected acceptance:\n{source}"
        );
        assert!(
            diagnostics.contains(message),
            "expected {message:?}, got {diagnostics}\n{source}"
        );
        return;
    }
    assert!(checked.status.success(), "{diagnostics}\n{source}");
    let output = project.run("--run");
    let diagnostics = String::from_utf8_lossy(&output.stderr);
    if let Some(message) = expectation.strip_prefix("fail:") {
        assert_eq!(output.status.code(), Some(1), "{diagnostics}\n{source}");
        assert!(
            diagnostics.contains(message),
            "expected {message:?}, got {diagnostics}\n{source}"
        );
    } else {
        let expected = expectation
            .strip_prefix("run:")
            .expect("unknown specification expectation");
        assert!(output.status.success(), "{diagnostics}\n{source}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert_eq!(stdout.trim(), expected, "{source}");
    }
}

#[test]
fn specification_examples() {
    let project = Project::new();
    let specification = include_str!("../../../docs/language-spec.md");
    let mut lines = specification.lines();
    let mut examples = 0;
    while let Some(line) = lines.next() {
        let Some(expectation) = line
            .strip_prefix("Example (`")
            .and_then(|line| line.strip_suffix("`):"))
        else {
            continue;
        };
        assert_eq!(lines.next(), Some(""));
        assert_eq!(lines.next(), Some("```nerv"));
        let mut source = String::new();
        let mut closed = false;
        for line in lines.by_ref() {
            if line == "```" {
                closed = true;
                break;
            }
            source.push_str(line);
            source.push('\n');
        }
        assert!(closed, "unclosed specification example");
        verify(&project, expectation, &source);
        examples += 1;
    }
    assert!(
        examples >= 30,
        "expected coverage of every language-rule section"
    );
}

#[test]
fn module_resolution_visibility_and_cycles() {
    let project = Project::new();
    project.write("lib/math.nerv", "module unrelated.label\nfn add(a: i64, b: i64) -> i64 => a + b\nexport fn answer() -> i64 => add(20, 22)\n");
    verify(
        &project,
        "run:42",
        "import lib.math\nfn main() -> i64 => answer()\n",
    );
    verify(
        &project,
        "run:42",
        "import lib.math\nimport lib.math\nfn main() -> i64 => add(20, 22)\n",
    );
    project.write(
        "lib/wrapper.nerv",
        "import math\nfn wrapped() -> i64 => answer()\n",
    );
    verify(
        &project,
        "run:42",
        "import lib.wrapper\nfn main() -> i64 => wrapped()\n",
    );
    verify(
        &project,
        "reject:duplicate function",
        "import lib.math\nfn answer() -> i64 => 1\nfn main() -> i64 => answer()\n",
    );
    verify(
        &project,
        "reject:unknown function",
        "fn main() -> i64 => answer()\n",
    );
    project.write("a.nerv", "import b\nfn a() -> i64 => 1\n");
    project.write("b.nerv", "import a\nfn b() -> i64 => 2\n");
    verify(
        &project,
        "reject:import cycle",
        "import a\nfn main() -> i64 => a()\n",
    );
    verify(
        &project,
        "reject:",
        "import missing\nfn main() -> i64 => 0\n",
    );
}

#[test]
fn numeric_aliases_have_the_documented_representation() {
    let project = Project::new();
    for name in [
        "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "isize", "usize",
    ] {
        verify(
            &project,
            "run:-256",
            &format!("fn main() -> i64\n    let value: {name} = -256\n    value\n"),
        );
    }
    for name in ["f32", "f64"] {
        verify(
            &project,
            "run:1",
            &format!(
                "fn main() -> i64\n    let value: {name} = 16777217.0\n    i64(value - 16777216.0)\n"
            ),
        );
    }
}
