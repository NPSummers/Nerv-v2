use std::{
    env,
    ffi::OsString,
    io::{self, BufRead, Write},
    process::{Command, ExitCode, Stdio},
    time::Instant,
};

#[derive(Debug, Default, PartialEq, Eq)]
struct Totals {
    passed: u64,
    failed: u64,
    ignored: u64,
    filtered: u64,
    suites: u64,
}

impl Totals {
    fn record(&mut self, line: &str) {
        let Some(summary) = line.strip_prefix("test result: ") else {
            return;
        };
        let Some((status, counts)) = summary.split_once(". ") else {
            return;
        };
        if !matches!(status, "ok" | "FAILED") {
            return;
        }
        let mut counts = counts.split(';').map(str::trim);
        let mut count =
            |suffix: &str| -> Option<u64> { counts.next()?.strip_suffix(suffix)?.parse().ok() };
        let (Some(passed), Some(failed), Some(ignored), Some(_measured), Some(filtered)) = (
            count(" passed"),
            count(" failed"),
            count(" ignored"),
            count(" measured"),
            count(" filtered out"),
        ) else {
            return;
        };
        self.passed += passed;
        self.failed += failed;
        self.ignored += ignored;
        self.filtered += filtered;
        self.suites += 1;
    }

    fn summary(&self, success: bool) -> String {
        let total = self.passed + self.failed + self.ignored;
        let result = if success && self.failed == 0 {
            "PASS"
        } else {
            "FAIL"
        };
        let score = if total == 0 || (!success && self.failed == 0) {
            "N/A (no completed tests or the run was interrupted by an error)".into()
        } else {
            format!(
                "{}/{} ({:.1}%)",
                self.passed,
                total,
                self.passed as f64 * 100.0 / total as f64
            )
        };
        let mut summary = format!(
            "\n=== Overall test results ===\nResult: {result}\nPassed: {} | Failed: {} | Ignored: {} | Filtered out: {}\nCompleted suites: {}\nOverall score: {score}\n",
            self.passed, self.failed, self.ignored, self.filtered, self.suites,
        );
        if !success {
            summary.push_str("Cargo reported an error. Counts cover completed suites only; some tests may not have run.\n");
        }
        summary
    }
}

fn cargo_args(args: impl IntoIterator<Item = OsString>) -> Vec<OsString> {
    let args = args.into_iter().collect::<Vec<_>>();
    let selected_packages = args.iter().take_while(|arg| *arg != "--").any(|arg| {
        arg.to_str().is_some_and(|arg| {
            arg == "-p"
                || arg == "--package"
                || arg.starts_with("--package=")
                || (arg.starts_with("-p") && arg.len() > 2)
                || arg == "--workspace"
                || arg == "--all"
        })
    });
    let mut command = ["test", "--locked", "--no-fail-fast", "--color=never"]
        .map(OsString::from)
        .to_vec();
    // --workspace together with -p selects their union, not just those packages.
    // In particular, Linux's frontend-only CI must not accidentally build LLVM.
    if !selected_packages {
        command.push("--workspace".into());
    }
    command.extend(args);
    command
}

fn run(args: impl IntoIterator<Item = OsString>) -> io::Result<i32> {
    let started = Instant::now();
    let mut child = Command::new("cargo")
        .args(cargo_args(args))
        .env("CARGO_TERM_COLOR", "never")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let mut totals = Totals::default();
    let stream_result = (|| -> io::Result<()> {
        let reader = io::BufReader::new(child.stdout.take().expect("piped Cargo output"));
        let mut stdout = io::stdout().lock();
        for line in reader.lines() {
            let line = line?;
            totals.record(&line);
            writeln!(stdout, "{line}")?;
            stdout.flush()?;
        }
        Ok(())
    })();
    if stream_result.is_err() {
        // Stop Cargo if its output can no longer be read.
        let _ = child.kill();
    }
    let status = child.wait()?;
    stream_result?;
    println!(
        "{}Elapsed: {:.2}s",
        totals.summary(status.success()),
        started.elapsed().as_secs_f64()
    );
    Ok(status.code().unwrap_or(1))
}

fn main() -> ExitCode {
    match run(env::args_os().skip(1)) {
        Ok(code) => ExitCode::from(code as u8),
        Err(error) => {
            eprintln!("Test runner error: {error}\nOverall score: N/A (test run did not complete)");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_success_failure_and_ignored_tests() {
        let mut totals = Totals::default();
        totals.record("test result: ok. 8 passed; 0 failed; 1 ignored; 0 measured; 2 filtered out; finished in 0.01s");
        totals.record("test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s");
        assert_eq!(
            totals,
            Totals {
                passed: 9,
                failed: 1,
                ignored: 1,
                filtered: 2,
                suites: 2
            }
        );
        let summary = totals.summary(false);
        assert!(summary.contains("Result: FAIL"));
        assert!(summary.contains("9/11 (81.8%)"));
        assert!(summary.contains("completed suites only"));
    }

    #[test]
    fn ignores_unrelated_or_malformed_output() {
        let mut totals = Totals::default();
        for line in [
            "running 5 tests",
            "test my_test ... ok",
            "test result: ok. invalid",
            "test result: unknown. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
            "  test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out",
        ] {
            totals.record(line);
        }
        assert_eq!(totals, Totals::default());
    }

    #[test]
    fn zero_tests_do_not_earn_a_perfect_score() {
        let mut totals = Totals::default();
        totals.record("test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 94 filtered out; finished in 0.00s");
        assert!(totals.summary(true).contains("Overall score: N/A"));
    }

    #[test]
    fn build_errors_and_crashes_do_not_earn_a_perfect_score() {
        let totals = Totals {
            passed: 5,
            suites: 1,
            ..Totals::default()
        };
        assert!(totals.summary(false).contains("Overall score: N/A"));
        assert!(totals.summary(false).contains("Result: FAIL"));
    }

    #[test]
    fn a_successful_run_earns_a_perfect_score() {
        let totals = Totals {
            passed: 94,
            suites: 8,
            ..Totals::default()
        };
        assert!(
            totals
                .summary(true)
                .contains("Overall score: 94/94 (100.0%)")
        );
    }

    #[test]
    fn preserves_filters_and_runs_remaining_suites_after_failures() {
        let forwarded = [
            "-p",
            "nerv",
            "--test",
            "diagnostics",
            "--",
            "--test-threads=1",
        ]
        .map(OsString::from);
        let args = cargo_args(forwarded.clone());
        assert!(args.contains(&OsString::from("--no-fail-fast")));
        assert!(!args.contains(&OsString::from("--workspace")));
        assert_eq!(&args[4..], &forwarded);
    }

    #[test]
    fn defaults_to_workspace_but_honors_each_package_selection_spelling() {
        assert!(cargo_args([]).contains(&OsString::from("--workspace")));
        for flag in ["-p", "--package", "--package=nerv", "-pnerv"] {
            assert!(!cargo_args([flag.into()]).contains(&OsString::from("--workspace")));
        }
        let explicit = cargo_args(["--workspace".into()]);
        assert_eq!(
            explicit.iter().filter(|arg| *arg == "--workspace").count(),
            1
        );
        assert!(cargo_args(["--".into(), "-p".into()]).contains(&OsString::from("--workspace")));
    }
}
