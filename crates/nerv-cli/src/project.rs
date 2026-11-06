use nerv_jit::{BuildMode, CpuTarget};
use nerv_sema::{ImportPaths, Program, check_path_with_imports};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};
use toml::Value;

#[derive(Clone)]
struct Package {
    root: PathBuf,
    name: String,
    version: String,
    entry: PathBuf,
    library: PathBuf,
    manifest: String,
    dependencies: BTreeMap<String, PathBuf>,
}

pub(crate) struct Project {
    root: PathBuf,
    packages: BTreeMap<PathBuf, Package>,
    imports: ImportPaths,
}

fn read(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn string(table: &toml::Table, key: &str) -> Result<String, String> {
    table
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("manifest requires string `{key}`"))
}

fn identifier(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .enumerate()
            .all(|(i, c)| c == b'_' || c.is_ascii_alphabetic() || (i > 0 && c.is_ascii_digit()))
}

fn keys(table: &toml::Table, allowed: &[&str]) -> Result<(), String> {
    for key in table.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!("unknown manifest key `{key}`"));
        }
    }
    Ok(())
}

fn source_path(root: &Path, path: &str) -> Result<PathBuf, String> {
    let path = Path::new(path);
    if path.is_absolute()
        || path.components().any(|c| {
            !matches!(
                c,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
    {
        return Err("package source paths must stay inside the package".into());
    }
    if path.extension().is_none_or(|s| s != "nerv") {
        return Err("package entry and lib must be .nerv files".into());
    }
    Ok(root.join(path))
}

impl Package {
    fn load(root: &Path) -> Result<Self, String> {
        let root = root
            .canonicalize()
            .map_err(|e| format!("{}: {e}", root.display()))?;
        let manifest = read(&root.join("Nerv.toml"))?;
        let document: toml::Table = manifest
            .parse()
            .map_err(|e| format!("{}: {e}", root.join("Nerv.toml").display()))?;
        keys(&document, &["package", "dependencies"])?;
        let package = document
            .get("package")
            .and_then(Value::as_table)
            .ok_or("manifest requires [package]")?;
        keys(package, &["name", "version", "entry", "lib"])?;
        let name = string(package, "name")?;
        if !identifier(&name) || name == "nerv" {
            return Err("package name must be an identifier other than `nerv`".into());
        }
        let version = string(package, "version")?;
        if version.split('.').count() != 3 || version.split('.').any(|s| s.parse::<u64>().is_err())
        {
            return Err("package version must have three numeric components".into());
        }
        let entry = source_path(
            &root,
            package
                .get("entry")
                .map(|v| v.as_str().ok_or("entry must be a string"))
                .transpose()?
                .unwrap_or("src/main.nerv"),
        )?;
        let library = source_path(
            &root,
            package
                .get("lib")
                .map(|v| v.as_str().ok_or("lib must be a string"))
                .transpose()?
                .unwrap_or("src/lib.nerv"),
        )?;
        let mut dependencies = BTreeMap::new();
        if let Some(deps) = document.get("dependencies") {
            for (alias, spec) in deps.as_table().ok_or("dependencies must be a table")? {
                if !identifier(alias) || alias == "nerv" || alias == &name {
                    return Err(format!("invalid dependency alias `{alias}`"));
                }
                let spec = spec
                    .as_table()
                    .ok_or("only local dependencies with { path = \"...\" } are supported")?;
                keys(spec, &["path"])?;
                dependencies.insert(alias.clone(), root.join(string(spec, "path")?));
            }
        }
        Ok(Self {
            root,
            name,
            version,
            entry,
            library,
            manifest,
            dependencies,
        })
    }
}

pub(crate) fn find_manifest(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|p| p.join("Nerv.toml"))
        .find(|p| p.is_file())
}

impl Project {
    fn load(manifest: &Path, locked: bool) -> Result<Self, String> {
        let root = manifest
            .canonicalize()
            .map_err(|e| format!("{}: {e}", manifest.display()))?
            .parent()
            .unwrap()
            .to_path_buf();
        let mut project = Self {
            root: root.clone(),
            packages: BTreeMap::new(),
            imports: ImportPaths::new(),
        };
        project.visit(&root, &mut BTreeSet::new())?;
        let lock = project.lock_text()?;
        let path = root.join("Nerv.lock");
        if fs::read_to_string(&path).ok().as_deref() != Some(&lock) {
            if locked {
                return Err("Nerv.lock is missing or stale; run without --locked to resolve local dependencies".into());
            }
            write_atomic(&path, lock.as_bytes())?;
        }
        Ok(project)
    }

    fn visit(&mut self, root: &Path, loading: &mut BTreeSet<PathBuf>) -> Result<(), String> {
        let package = Package::load(root)?;
        if loading.contains(&package.root) {
            return Err(format!("dependency cycle at {}", package.root.display()));
        }
        if self.packages.contains_key(&package.root) {
            return Ok(());
        }
        loading.insert(package.root.clone());
        let mut aliases = BTreeMap::new();
        for (alias, path) in &package.dependencies {
            self.visit(path, loading)?;
            let dependency = &self.packages[&path.canonicalize().map_err(|e| e.to_string())?];
            if !dependency.library.is_file() {
                return Err(format!(
                    "dependency `{alias}` needs {}",
                    dependency.library.display()
                ));
            }
            aliases.insert(alias.clone(), dependency.library.clone());
        }
        if package.library.is_file() {
            aliases.insert(package.name.clone(), package.library.clone());
        }
        self.imports.insert(package.root.clone(), aliases);
        loading.remove(&package.root);
        self.packages.insert(package.root.clone(), package);
        Ok(())
    }

    fn lock_text(&self) -> Result<String, String> {
        let mut packages = Vec::new();
        for package in self.packages.values() {
            let mut table = toml::Table::new();
            table.insert("name".into(), Value::String(package.name.clone()));
            table.insert("version".into(), Value::String(package.version.clone()));
            table.insert(
                "manifest".into(),
                Value::String(relative(&self.root, &package.root.join("Nerv.toml"))?),
            );
            table.insert(
                "checksum".into(),
                Value::String(hash(package.manifest.as_bytes())),
            );
            let mut dependencies = toml::Table::new();
            for (alias, path) in &package.dependencies {
                dependencies.insert(
                    alias.clone(),
                    Value::String(relative(
                        &self.root,
                        &path
                            .canonicalize()
                            .map_err(|e| e.to_string())?
                            .join("Nerv.toml"),
                    )?),
                );
            }
            table.insert("dependencies".into(), Value::Table(dependencies));
            packages.push(Value::Table(table));
        }
        packages.sort_by_key(|v| v["manifest"].as_str().unwrap().to_owned());
        let mut lock = toml::Table::new();
        lock.insert("version".into(), Value::Integer(1));
        lock.insert("package".into(), Value::Array(packages));
        toml::to_string_pretty(&lock).map_err(|e| e.to_string())
    }

    fn package(&self) -> &Package {
        &self.packages[&self.root]
    }

    fn entry(&self) -> Result<PathBuf, String> {
        let package = self.package();
        if package.entry.is_file() {
            Ok(package.entry.clone())
        } else if package.library.is_file() {
            Ok(package.library.clone())
        } else {
            Err("package needs src/main.nerv or src/lib.nerv".into())
        }
    }

    fn sources(&self) -> Result<Vec<PathBuf>, String> {
        let mut sources = vec![self.entry()?];
        let library = &self.package().library;
        if library.is_file() && !sources.contains(library) {
            sources.push(library.clone());
        }
        collect_sources(&self.root.join("src"), &mut sources)?;
        collect_sources(&self.root.join("tests"), &mut sources)?;
        let mut sources = sources
            .into_iter()
            .map(|p| p.canonicalize().map_err(|e| e.to_string()))
            .collect::<Result<Vec<_>, _>>()?;
        sources.sort();
        sources.dedup();
        Ok(sources)
    }
}

fn relative(root: &Path, path: &Path) -> Result<String, String> {
    let root: Vec<_> = root.components().collect();
    let path: Vec<_> = path.components().collect();
    let common = root.iter().zip(&path).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return Err(
            "local dependencies must share a filesystem volume for a portable lockfile".into(),
        );
    }
    let mut result = PathBuf::new();
    for _ in common..root.len() {
        result.push("..");
    }
    for component in &path[common..] {
        result.push(component.as_os_str());
    }
    Ok(result.to_string_lossy().replace('\\', "/"))
}

pub(crate) fn collect_sources(path: &Path, sources: &mut Vec<PathBuf>) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    let mut entries = fs::read_dir(path)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            collect_sources(&entry.path(), sources)?;
        } else if entry.path().extension().is_some_and(|s| s == "nerv") {
            sources.push(entry.path());
        }
    }
    Ok(())
}

pub(crate) fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
    let result = fs::rename(&temporary, path).map_err(|e| format!("{}: {e}", path.display()));
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

#[derive(Default)]
struct Options {
    source: Option<PathBuf>,
    manifest: Option<PathBuf>,
    output: Option<PathBuf>,
    release: bool,
    native: bool,
    locked: bool,
    check_format: bool,
    no_cache: bool,
    arguments: Vec<OsString>,
}

fn options(command: &str, args: Vec<OsString>) -> Result<Options, String> {
    let mut options = Options::default();
    let mut selected_mode = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--") if command == "run" => {
                options.arguments.extend(args);
                break;
            }
            Some("--release") if matches!(command, "build" | "run") => {
                if selected_mode {
                    return Err("choose only one of --debug and --release".into());
                }
                selected_mode = true;
                options.release = true;
            }
            Some("--debug") if matches!(command, "build" | "run") => {
                if selected_mode {
                    return Err("choose only one of --debug and --release".into());
                }
                selected_mode = true;
                options.release = false;
            }
            Some("--locked") => options.locked = true,
            Some("--no-cache") if matches!(command, "build" | "run") => options.no_cache = true,
            Some("--manifest-path") => {
                options.manifest = Some(args.next().ok_or("--manifest-path needs a path")?.into())
            }
            Some("-o") if command == "build" => {
                options.output = Some(args.next().ok_or("-o requires an output path")?.into())
            }
            Some("--cpu") if matches!(command, "build" | "run") => {
                options.native = match args.next().and_then(|s| s.into_string().ok()).as_deref() {
                    Some("native") => true,
                    Some("generic") => false,
                    _ => return Err("--cpu requires generic or native".into()),
                };
            }
            Some("--check") if command == "fmt" => options.check_format = true,
            Some(arg) if arg.starts_with('-') => {
                return Err(format!("unknown {command} option `{arg}`"));
            }
            _ => {
                if options.source.replace(arg.into()).is_some() {
                    return Err(format!("{command} accepts one source path"));
                }
            }
        }
    }
    Ok(options)
}

pub(crate) fn command(command: &str, args: Vec<OsString>) -> ExitCode {
    match execute(command, args) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn execute(command: &str, args: Vec<OsString>) -> Result<u8, String> {
    if args
        .iter()
        .take_while(|s| *s != "--")
        .any(|s| s == "--help" || s == "-h")
    {
        println!(
            "nerv {command} [file.nerv] [--manifest-path Nerv.toml] [--locked]\nBuild/run: [--debug|--release] [--cpu generic|native]; build: [-o output]; run: [-- arguments]; fmt: [--check]"
        );
        return Ok(0);
    }
    if command == "new" {
        return new(args);
    }
    let options = options(command, args)?;
    let cwd = env::current_dir().map_err(|e| e.to_string())?;
    let manifest = options.manifest.clone().or_else(|| find_manifest(&cwd));
    let project = manifest
        .as_ref()
        .map(|m| Project::load(m, options.locked))
        .transpose()?;
    if options.locked && project.is_none() {
        return Err("--locked requires a project".into());
    }
    let source = options
        .source
        .clone()
        .or_else(|| project.as_ref().and_then(|p| p.entry().ok()))
        .ok_or(if command == "build" { "usage: nerv build [file.nerv] [--release] [-o output]\nno source file or Nerv.toml found" } else { "no source file or Nerv.toml found" })?;
    let imports = project
        .as_ref()
        .map(|p| p.imports.clone())
        .unwrap_or_default();
    if command == "fmt" {
        let mut paths = Vec::new();
        if options.source.is_some() {
            paths.push(source);
        } else if let Some(project) = &project {
            collect_sources(&project.root.join("src"), &mut paths)?;
            collect_sources(&project.root.join("tests"), &mut paths)?;
        }
        return crate::format::files(&paths, options.check_format);
    }
    if command == "__test" {
        let program = check_path_with_imports(&source, &imports).map_err(|e| e.to_string())?;
        let source_id = program.modules.last().unwrap().source;
        let count =
            nerv_jit::run_tests_in_source(&program.ir, source_id).map_err(|e| e.to_string())?;
        println!("{count} tests passed ({})", source.display());
        return Ok(0);
    }
    if command == "test" {
        let sources = if options.source.is_some() {
            vec![source]
        } else {
            project
                .as_ref()
                .ok_or("test needs a project or source file")?
                .sources()?
        };
        let mut failures = 0;
        for source in &sources {
            let mut child = Command::new(env::current_exe().map_err(|e| e.to_string())?);
            child.arg("__test").arg(source);
            if let Some(manifest) = &manifest {
                child.arg("--manifest-path").arg(manifest).arg("--locked");
            }
            if !child.status().map_err(|e| e.to_string())?.success() {
                failures += 1;
            }
        }
        println!(
            "{} test files passed; {failures} failed",
            sources.len() - failures
        );
        return Ok(u8::from(failures > 0));
    }
    if command == "check" {
        let sources = if options.source.is_some() {
            vec![source]
        } else {
            project.as_ref().unwrap().sources()?
        };
        for source in sources {
            check_path_with_imports(&source, &imports).map_err(|e| e.to_string())?;
        }
        println!("Type check passed");
        return Ok(0);
    }
    let mut program = check_path_with_imports(&source, &imports).map_err(|e| e.to_string())?;
    program.ir.set_process_entry(true);
    if program.ir.main().is_none() {
        return Err(
            "build/run require a main function; use check/test for library packages".into(),
        );
    }
    let root = project.as_ref().map(|p| p.root.clone()).unwrap_or(
        source
            .canonicalize()
            .map_err(|e| e.to_string())?
            .parent()
            .unwrap()
            .to_path_buf(),
    );
    let name = project
        .as_ref()
        .map(|p| p.package().name.clone())
        .unwrap_or(source.file_stem().unwrap().to_string_lossy().into_owned());
    let mut output = root
        .join("target/nerv")
        .join(if options.release { "release" } else { "debug" })
        .join(name);
    if cfg!(windows) {
        output.set_extension("exe");
    }
    if let Some(path) = &options.output {
        output = cwd.join(path);
    }
    if let Some(project) = &project {
        for package in project.packages.values() {
            for protected in [
                package.root.join("Nerv.toml"),
                package.root.join("Nerv.lock"),
            ] {
                if output.canonicalize().ok().as_ref() == protected.canonicalize().ok().as_ref()
                    && protected.exists()
                {
                    return Err("output would overwrite a project manifest or lockfile".into());
                }
            }
        }
    }
    if output
        .extension()
        .is_some_and(|e| e == "nerv-cache" || e == "nerv-object" || e == "nerv-build-lock")
    {
        return Err("output extension is reserved for compiler cache files".into());
    }
    fs::create_dir_all(output.parent().unwrap()).map_err(|e| e.to_string())?;
    let build_lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(output.with_extension("nerv-build-lock"))
        .map_err(|e| e.to_string())?;
    build_lock.lock().map_err(|e| e.to_string())?;
    cached_build(program, &output, &options, project.as_ref())?;
    if command == "run" {
        let status = Command::new(&output)
            .args(&options.arguments)
            .status()
            .map_err(|e| format!("could not run {}: {e}", output.display()))?;
        return Ok(status.code().map(|c| c.rem_euclid(256) as u8).unwrap_or(1));
    }
    Ok(0)
}

fn cached_build(
    program: Program,
    output: &Path,
    options: &Options,
    project: Option<&Project>,
) -> Result<(), String> {
    let mut fingerprint = Sha256::new();
    let mut feed = |bytes: &[u8]| {
        fingerprint.update((bytes.len() as u64).to_le_bytes());
        fingerprint.update(bytes);
    };
    let executable = env::current_exe().map_err(|e| e.to_string())?;
    feed(&fs::read(&executable).map_err(|e| e.to_string())?);
    for name in [
        "LLVM_SYS_231_PREFIX",
        "LLVM_CONFIG_PATH",
        "DYLD_LIBRARY_PATH",
        "DYLD_FALLBACK_LIBRARY_PATH",
        "LD_LIBRARY_PATH",
    ] {
        feed(format!("{name}={:?}", env::var_os(name)).as_bytes());
    }
    let mut libraries = vec![executable.with_file_name("libLLVM-23.dylib")];
    if let Some(prefix) = env::var_os("LLVM_SYS_231_PREFIX") {
        libraries.push(PathBuf::from(prefix).join("lib/libLLVM-23.dylib"));
    }
    for library in libraries {
        if library.is_file() {
            feed(&fs::read(library).map_err(|e| e.to_string())?);
        }
    }

    let dll = executable.with_file_name("LLVM-C.dll");
    if dll.is_file() {
        feed(&fs::read(dll).map_err(|e| e.to_string())?);
    }
    feed(
        format!(
            "{:?}|{}|{}|{}",
            options.release,
            options.native,
            env::consts::OS,
            env::consts::ARCH
        )
        .as_bytes(),
    );
    if options.native {
        feed(nerv_jit::host_cpu_identity()?.as_bytes());
    }
    for source in &program.sources.files {
        feed(source.path.as_os_str().as_encoded_bytes());
        feed(source.text.as_bytes());
    }
    if let Some(project) = project {
        feed(project.lock_text()?.as_bytes());
    }
    let fingerprint = format!("{:x}", fingerprint.finalize());
    let stamp = output.with_extension("nerv-cache");
    let object = output.with_extension("nerv-object");
    let reusable = !options.no_cache
        && fs::read(&object).is_ok_and(|bytes| {
            fs::read_to_string(&stamp).ok().as_deref()
                == Some(&format!("{fingerprint}\n{}\n", hash(&bytes)))
        });
    if reusable {
        eprintln!("Fresh code generation for {}", output.display());
    }
    crate::build::compile_cached(
        program,
        output,
        if options.release {
            BuildMode::Release
        } else {
            BuildMode::Debug
        },
        if options.native {
            CpuTarget::Native
        } else {
            CpuTarget::Generic
        },
        false,
        Some((&object, reusable)),
    )?;
    write_atomic(
        &stamp,
        format!(
            "{fingerprint}\n{}\n",
            hash(&fs::read(object).map_err(|e| e.to_string())?)
        )
        .as_bytes(),
    )
}

fn new(args: Vec<OsString>) -> Result<u8, String> {
    let library = args.iter().any(|s| s == "--lib");
    let paths: Vec<_> = args.into_iter().filter(|s| s != "--lib").collect();
    if paths.len() != 1 {
        return Err("usage: nerv new [--lib] PATH".into());
    }
    let path = PathBuf::from(&paths[0]);
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("invalid project path")?;
    if !identifier(name) || name == "nerv" {
        return Err("project name must be an identifier other than nerv".into());
    }
    fs::create_dir(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    fs::create_dir(path.join("src")).map_err(|e| e.to_string())?;
    fs::write(
        path.join("Nerv.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\n[dependencies]\n"),
    )
    .map_err(|e| e.to_string())?;
    fs::write(path.join(".gitignore"), "/target/\n").map_err(|e| e.to_string())?;
    let source = if library {
        "fn answer() -> i64 => 42\n\ntest \"answer\"\n    assert(answer() == 42)\n"
    } else {
        "fn main() -> i64\n    println(\"Hello from Nerv\")\n    0\n\ntest \"arithmetic\"\n    assert(20 + 22 == 42)\n"
    };
    fs::write(
        path.join(if library {
            "src/lib.nerv"
        } else {
            "src/main.nerv"
        }),
        source,
    )
    .map_err(|e| e.to_string())?;
    Project::load(&path.join("Nerv.toml"), false)?;
    println!(
        "Created {} at {}",
        if library { "library" } else { "application" },
        path.display()
    );
    Ok(0)
}
