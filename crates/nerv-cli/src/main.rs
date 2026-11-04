use std::{env, fs, process::ExitCode, time::Instant};

use nerv_jit::{run_benches, run_main, run_tests};
use nerv_lexer::Lexer;
use nerv_sema::check_path;
use nerv_syntax::{SourceFile, SourceId, SourceMap, Span, parse};

mod build;

fn report_backend_error(error: nerv_jit::Error, sources: &SourceMap) {
    if let Some(span) = error.span {
        eprintln!(
            "{}",
            sources.get(span.source).render(span, &error.message, &[])
        );
    } else {
        // Runtime failures and LLVM infrastructure errors may have no source span.
        eprintln!("{error}");
    }
}

fn main() -> ExitCode {
    let mut args = env::args_os();
    let program = args.next().unwrap_or_default();
    let Some(path) = args.next() else {
        eprintln!(
            "usage: {} <file.nerv> [--lex|--parse|--check|--run|--test|--bench]\n       nerv build <file.nerv> [-o output] [--debug|--release] [--emit-object]",
            program.to_string_lossy()
        );
        return ExitCode::FAILURE;
    };
    if path == "build" {
        return build::command(args);
    }
    let mode = args.next().map(|arg| arg.to_string_lossy().into_owned());
    if args.next().is_some() {
        eprintln!(
            "usage: {} <file.nerv> [--lex|--parse|--check|--run|--test|--bench]\n       nerv build <file.nerv> [-o output] [--debug|--release] [--emit-object]",
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

    let source_file = SourceFile {
        id: SourceId(0),
        path: path.clone().into(),
        text: source.clone().into(),
    };
    let tokens: Vec<_> = match Lexer::new(&source).collect() {
        Ok(tokens) => tokens,
        Err(error) => {
            let span = Span {
                source: source_file.id,
                start: error.span.start,
                end: error.span.end,
            };
            eprintln!("{}", source_file.render(span, &error.to_string(), &[]));
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
                eprintln!(
                    "{}",
                    source_file.render(error.span(source_file.id), &error.to_string(), &[])
                );
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
        match run_main(&program.ir) {
            Ok(value) => println!("{value}"),
            Err(error) => {
                report_backend_error(error, &program.sources);
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
        match run_tests(&program.ir) {
            Ok(count) => println!("{count} tests passed"),
            Err(error) => {
                report_backend_error(error, &program.sources);
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
        let started = Instant::now();
        match run_benches(&program.ir) {
            Ok(count) => println!(
                "{count} benchmarks ran in {:.3} ms",
                started.elapsed().as_secs_f64() * 1000.0
            ),
            Err(error) => {
                report_backend_error(error, &program.sources);
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
