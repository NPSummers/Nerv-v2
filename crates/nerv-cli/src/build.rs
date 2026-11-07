use nerv_jit::{BuildMode, CpuTarget, emit_object_for_cpu};
use nerv_sema::check_path;
use std::{
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const RUNTIME: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/runtime.archive"));
const NATIVE_LIBS: &str = include_str!(concat!(env!("OUT_DIR"), "/runtime-link-libs.txt"));

struct Staging(PathBuf);
impl Staging {
    fn new(parent: &Path) -> Result<Self, String> {
        for _ in 0..100 {
            let path = parent.join(format!(
                ".nerv-build-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let builder = fs::DirBuilder::new();
            #[cfg(unix)]
            let builder = {
                use std::os::unix::fs::DirBuilderExt;
                let mut builder = builder;
                builder.mode(0o700);
                builder
            };
            match builder.create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(format!("could not create build staging directory: {error}"));
                }
            }
        }
        Err("could not allocate a unique build directory".into())
    }
}
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn command(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    match build(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn build(args: impl IntoIterator<Item = OsString>) -> Result<(), String> {
    let mut args = args.into_iter();
    let mut source = None;
    let mut output = None;
    let mut selected_mode = None;
    let mut object_only = false;
    let mut selected_cpu = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("-h" | "--help") => {
                println!(
                    "usage: nerv build <file.nerv> [-o <output>] [--debug|--release] [--cpu generic|native] [--emit-object]"
                );
                return Ok(());
            }
            Some("-o") => {
                if output.is_some() {
                    return Err("-o specified more than once".into());
                }
                output = Some(PathBuf::from(
                    args.next().ok_or("-o requires an output path")?,
                ));
            }
            Some("--debug" | "--release") => {
                let mode = if arg == "--release" {
                    BuildMode::Release
                } else {
                    BuildMode::Debug
                };
                if selected_mode.replace(mode).is_some() {
                    return Err("choose only one of --debug and --release".into());
                }
            }
            Some("--emit-object") => object_only = true,
            Some("--cpu") => {
                let cpu = args.next().ok_or("--cpu requires generic or native")?;
                if selected_cpu.replace(parse_cpu(&cpu)?).is_some() {
                    return Err("CPU target specified more than once".into());
                }
            }
            Some(arg) if arg.starts_with("--cpu=") => {
                if selected_cpu
                    .replace(parse_cpu(arg.trim_start_matches("--cpu=").as_ref())?)
                    .is_some()
                {
                    return Err("CPU target specified more than once".into());
                }
            }
            Some(arg) if arg.starts_with('-') => {
                return Err(format!("unknown build option `{arg}`"));
            }
            _ => {
                if source.replace(PathBuf::from(arg)).is_some() {
                    return Err("build accepts one source file".into());
                }
            }
        }
    }
    let source = source
        .ok_or("usage: nerv build <file.nerv> [-o <output>] [--debug|--release] [--cpu generic|native] [--emit-object]")?;
    let mode = selected_mode.unwrap_or(BuildMode::Debug);
    let cpu = selected_cpu.unwrap_or(CpuTarget::Generic);
    let output = output.unwrap_or_else(|| {
        let mut name = PathBuf::from(source.file_stem().unwrap_or_default());
        if object_only {
            name.set_extension(if cfg!(windows) { "obj" } else { "o" });
        } else if cfg!(windows) {
            name.set_extension("exe");
        }
        name
    });
    let output = env::current_dir().map_err(|e| e.to_string())?.join(output);
    let program = check_path(&source).map_err(|error| error.to_string())?;
    compile_program(program, &output, mode, cpu, object_only)
}

pub(crate) fn compile_program(
    program: nerv_sema::Program,
    output: &Path,
    mode: BuildMode,
    cpu: CpuTarget,
    object_only: bool,
) -> Result<(), String> {
    compile_cached(program, output, mode, cpu, object_only, None)
}

pub(crate) fn compile_cached(
    program: nerv_sema::Program,
    output: &Path,
    mode: BuildMode,
    cpu: CpuTarget,
    object_only: bool,
    cache: Option<(&Path, bool)>,
) -> Result<(), String> {
    if program
        .sources
        .files
        .iter()
        .any(|file| output.canonicalize().ok().as_ref() == Some(&file.path))
    {
        return Err("output would overwrite a source file".into());
    }
    if output
        .extension()
        .is_some_and(|extension| extension == "nerv")
    {
        return Err("output must not have the .nerv source extension".into());
    }
    let staging = Staging::new(output.parent().ok_or("invalid output path")?)?;
    let object = staging.0.join(if cfg!(windows) {
        "program.obj"
    } else {
        "program.o"
    });
    if let Some((cached, true)) = cache {
        fs::copy(cached, &object).map_err(|e| e.to_string())?;
    } else {
        emit_object_for_cpu(&program.ir, &object, mode, cpu).map_err(|error| {
            if let Some(span) = error.span {
                program
                    .sources
                    .get(span.source)
                    .render(span, &error.message, &[])
            } else {
                error.to_string()
            }
        })?;
        if let Some((cached, _)) = cache {
            crate::project::write_atomic(cached, &fs::read(&object).map_err(|e| e.to_string())?)?;
        }
    }
    if object_only {
        fs::rename(&object, &output).map_err(|error| format!("could not save object: {error}"))?;
    } else {
        let archive = staging.0.join(if cfg!(windows) {
            "runtime.lib"
        } else {
            "runtime.a"
        });
        fs::write(&archive, RUNTIME)
            .map_err(|error| format!("could not extract runtime archive: {error}"))?;
        let executable = staging
            .0
            .join(if cfg!(windows) { "app.exe" } else { "app" });
        let mut linker = if let Some(command) =
            crate::toolchain::link_command(&object, &archive, &executable, NATIVE_LIBS)?
        {
            command
        } else {
            let mut linker = linker()?;
            if cfg!(windows) && env!("NERV_BUILD_TARGET").contains("windows-msvc") {
                linker.args([
                    OsString::from("/NOLOGO"),
                    OsString::from("/SUBSYSTEM:CONSOLE"),
                    OsString::from(format!("/OUT:{}", executable.display())),
                ]);
            } else {
                linker.arg("-o").arg(&executable);
            }
            linker.arg(&object).arg(&archive).args(NATIVE_LIBS.lines());
            linker
        };
        let result = linker.output().map_err(|error| format!("could not run native linker (install native build tools or set NERV_LINKER): {error}"))?;
        if !result.status.success() {
            return Err(format!(
                "native link failed ({}):\n{}{}",
                result.status,
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            ));
        }
        fs::rename(&executable, &output)
            .map_err(|error| format!("could not save executable: {error}"))?;
    }
    eprintln!(
        "Built {} ({}, CPU {})",
        output.display(),
        if mode == BuildMode::Debug {
            "debug"
        } else {
            "release"
        },
        if cpu == CpuTarget::Native {
            "native"
        } else {
            "generic"
        }
    );
    Ok(())
}

fn parse_cpu(value: &std::ffi::OsStr) -> Result<CpuTarget, String> {
    match value.to_str() {
        Some("generic") => Ok(CpuTarget::Generic),
        Some("native") => Ok(CpuTarget::Native),
        _ => Err("CPU target must be generic or native".into()),
    }
}

pub(crate) fn linker() -> Result<Command, String> {
    if let Some(path) = env::var_os("NERV_LINKER") {
        return Ok(Command::new(path));
    }
    #[cfg(windows)]
    {
        if !env!("NERV_BUILD_TARGET").contains("windows-msvc") {
            return Err(
                "this compiler requires its bundled toolchain; set NERV_TOOLCHAIN to its directory"
                    .into(),
            );
        }
        find_msvc_tools::find(env!("NERV_BUILD_TARGET"), "link.exe").ok_or_else(|| "could not find MSVC link.exe; install Visual Studio C++ Build Tools or set NERV_LINKER".into())
    }
    #[cfg(not(windows))]
    {
        Ok(Command::new(
            env::var_os("CC").unwrap_or_else(|| "cc".into()),
        ))
    }
}

pub(crate) fn link_info() -> String {
    let mut info = toml::map::Map::new();
    info.insert(
        "target".into(),
        toml::Value::String(env!("NERV_BUILD_TARGET").into()),
    );
    info.insert(
        "libraries".into(),
        toml::Value::Array(
            NATIVE_LIBS
                .lines()
                .map(|line| toml::Value::String(line.into()))
                .collect(),
        ),
    );
    toml::to_string(&toml::Value::Table(info)).expect("link information is serializable")
}
