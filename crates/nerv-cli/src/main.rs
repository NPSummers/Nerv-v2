use std::{env, fs, process::ExitCode, time::Instant};

use nerv_jit::{run_benches, run_main, run_tests};
use nerv_lexer::Lexer;
use nerv_sema::check_path;
use nerv_syntax::parse;

fn main() -> ExitCode {
    let mut args = env::args_os();
    let program = args.next().unwrap_or_default();
    let Some(path) = args.next() else {
        eprintln!(
            "usage: {} <file.nerv> [--lex|--parse|--check|--run|--test|--bench]",
            program.to_string_lossy()
        );
        return ExitCode::FAILURE;
    };
    let mode = args.next().map(|arg| arg.to_string_lossy().into_owned());
    if args.next().is_some() {
        eprintln!(
            "usage: {} <file.nerv> [--lex|--parse|--check|--run|--test|--bench]",
            program.to_string_lossy()
        );
        return ExitCode::FAILURE;
    }

    let source = match fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("{}: {error}", path.to_string_lossy());
            return ExitCode::FAILURE;
        }
    };

    let tokens: Vec<_> = match Lexer::new(&source).collect() {
        Ok(tokens) => tokens,
        Err(error) => {
            eprintln!(
                "{}:{}:{}: {error}",
                path.to_string_lossy(),
                error.line,
                error.column
            );
            return ExitCode::FAILURE;
        }
    };

    if mode.as_deref() == Some("--lex") {
        for token in tokens {
            println!(
                "{}:{} {:?} {}",
                token.line,
                token.column,
                token.kind,
                token.text(&source)
            );
        }
        return ExitCode::SUCCESS;
    }

    if mode.as_deref() == Some("--parse") {
        match parse(&source) {
            Ok(module) => println!("{module:#?}"),
            Err(error) => {
                eprintln!("{}: {error}", path.to_string_lossy());
                return ExitCode::FAILURE;
            }
        }
        return ExitCode::SUCCESS;
    }

    if mode.as_deref() == Some("--check") {
        match check_path(&path) {
            Ok(_) => println!("Type check passed"),
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        }
        return ExitCode::SUCCESS;
    }

    if mode.as_deref() == Some("--run") {
        let program = match check_path(&path) {
            Ok(program) => program,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        };
        let modules = program
            .modules
            .iter()
            .map(|module| module.module.clone())
            .collect::<Vec<_>>();
        match run_main(&modules) {
            Ok(value) => println!("{value}"),
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        }
        return ExitCode::SUCCESS;
    }

    if mode.as_deref() == Some("--test") {
        let program = match check_path(&path) {
            Ok(program) => program,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        };
        let modules = program
            .modules
            .iter()
            .map(|module| module.module.clone())
            .collect::<Vec<_>>();
        match run_tests(&modules) {
            Ok(count) => println!("{count} tests passed"),
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        }
        return ExitCode::SUCCESS;
    }

    if mode.as_deref() == Some("--bench") {
        let program = match check_path(&path) {
            Ok(program) => program,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        };
        let modules = program
            .modules
            .iter()
            .map(|module| module.module.clone())
            .collect::<Vec<_>>();
        let started = Instant::now();
        match run_benches(&modules) {
            Ok(count) => println!(
                "{count} benchmarks ran in {:.3} ms",
                started.elapsed().as_secs_f64() * 1000.0
            ),
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        }
        return ExitCode::SUCCESS;
    }

    eprintln!(
        "the Rust frontend currently requires --lex, --parse, --check, --run, --test, or --bench"
    );
    ExitCode::FAILURE
}
