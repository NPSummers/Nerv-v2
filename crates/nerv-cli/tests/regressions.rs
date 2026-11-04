use std::{fs, path::Path, process::Command};

fn verify(case: &str) {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(case);
    let expectation = fs::read_to_string(directory.join("expect.txt")).unwrap();
    let mut lines = expectation.lines();
    let mode = lines.next().expect("fixture mode");
    assert!(matches!(mode, "--parse" | "--check" | "--run"));
    let status: i32 = lines.next().expect("fixture exit code").parse().unwrap();
    assert!(
        lines.next().is_none(),
        "unexpected fixture metadata: {case}"
    );
    let output = Command::new(env!("CARGO_BIN_EXE_nerv"))
        .current_dir(&directory)
        .args(["main.nerv", mode])
        .output()
        .unwrap();
    // Normalize only platform line endings, not meaningful whitespace.
    let stdout = String::from_utf8(output.stdout)
        .unwrap()
        .replace("\r\n", "\n");
    let stderr = String::from_utf8(output.stderr)
        .unwrap()
        .replace("\r\n", "\n");
    assert_eq!(
        output.status.code(),
        Some(status),
        "{case}:\n{stdout}\n{stderr}"
    );
    let expected = fs::read_to_string(directory.join("stdout.txt")).unwrap();
    if mode == "--parse" && status == 0 {
        assert!(
            !expected.trim().is_empty(),
            "empty parser expectation: {case}"
        );
        assert!(stdout.contains(expected.trim_end()), "{case}: {stdout}");
    } else {
        assert_eq!(stdout, expected.replace("\r\n", "\n"), "{case}");
    }
    if status == 0 {
        assert!(stderr.is_empty(), "{case}: {stderr}");
    } else {
        let expected = fs::read_to_string(directory.join("stderr.txt")).unwrap();
        assert!(
            !expected.trim().is_empty(),
            "empty diagnostic expectation: {case}"
        );
        assert!(stderr.contains(expected.trim_end()), "{case}: {stderr}");
    }
}

macro_rules! fixtures {
    ($($name:ident => $case:literal),+ $(,)?) => {
        $(#[test]
        fn $name() {
            verify($case);
        })+
    };
}

fixtures! {
    parses_without_type_checking => "parse/unresolved_name",
    rejects_malformed_syntax => "parse/missing_expression",
    checks_without_running => "check/no_execution",
    rejects_type_mismatch => "check/type_mismatch",
    rejects_unknown_name => "check/unknown_name",
    runs_structs => "run/structs",
    runs_enums => "run/enums",
    runs_traits_and_methods => "run/traits_methods",
    runs_arrays => "run/arrays",
    runs_lambdas => "run/lambdas",
    runs_imports => "run/imports",
    resolves_nominal_method_dispatch => "run/resolved_dispatch",
    calls_inferred_and_returned_functions => "run/indirect_calls",
    preserves_field_evaluation_order => "run/field_order",
    preserves_print_output_and_exit_code => "run/output",
    rejects_import_cycles => "check/import_cycle",
    reports_bounds_failure => "fail/bounds",
    reports_division_by_zero => "fail/division",
    reports_assertion_failure => "fail/assertion",
}
