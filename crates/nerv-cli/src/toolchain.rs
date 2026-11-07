use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

pub(crate) fn link_command(
    object: &Path,
    runtime: &Path,
    output: &Path,
    libraries: &str,
) -> Result<Option<Command>, String> {
    let root = if let Some(root) = env::var_os("NERV_TOOLCHAIN") {
        PathBuf::from(root)
    } else {
        let executable = env::current_exe()
            .and_then(|path| path.canonicalize())
            .map_err(|error| format!("cannot resolve compiler executable: {error}"))?;
        let root = executable
            .parent()
            .ok_or("compiler path has no parent")?
            .join("toolchain");
        if !root.exists() {
            if executable
                .parent()
                .is_some_and(|parent| parent.join("release-info.json").is_file())
            {
                return Err("release compiler is missing its bundled toolchain directory".into());
            }
            return Ok(None);
        }
        root
    };
    let root = root
        .canonicalize()
        .map_err(|error| format!("cannot find bundled toolchain: {error}"))?;
    let source = fs::read_to_string(root.join("Nerv-toolchain.toml"))
        .map_err(|error| format!("cannot read bundled toolchain manifest: {error}"))?;
    let plan = Plan::parse(&source, env!("NERV_BUILD_TARGET"))?;
    let executable = root
        .join(&plan.linker)
        .canonicalize()
        .map_err(|error| format!("bundled linker is missing: {error}"))?;
    if !executable.starts_with(&root) {
        return Err("bundled linker escapes toolchain directory".into());
    }
    let mut command = Command::new(
        env::var_os("NERV_LINKER")
            .map(PathBuf::from)
            .unwrap_or(executable),
    );
    command.args(plan.arguments(&root, object, runtime, output, libraries)?);
    for key in [
        "LIB",
        "LIBPATH",
        "INCLUDE",
        "SDKROOT",
        "DEVELOPER_DIR",
        "CPATH",
        "LIBRARY_PATH",
        "C_INCLUDE_PATH",
        "CPLUS_INCLUDE_PATH",
    ] {
        command.env_remove(key);
    }
    Ok(Some(command))
}

struct Plan {
    linker: PathBuf,
    arguments: Vec<String>,
}

impl Plan {
    fn parse(source: &str, target: &str) -> Result<Self, String> {
        let manifest: toml::Value = source
            .parse()
            .map_err(|error| format!("invalid toolchain manifest: {error}"))?;
        if manifest.get("version").and_then(toml::Value::as_integer) != Some(1) {
            return Err("unsupported toolchain manifest version".into());
        }
        if manifest.get("target").and_then(toml::Value::as_str) != Some(target) {
            return Err("bundled toolchain target does not match this compiler".into());
        }
        let linker = PathBuf::from(
            manifest
                .get("linker")
                .and_then(toml::Value::as_str)
                .ok_or("toolchain manifest needs a linker")?,
        );
        if linker.as_os_str().is_empty()
            || linker
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err("toolchain linker must be a relative path within its directory".into());
        }
        let arguments = manifest
            .get("arguments")
            .and_then(toml::Value::as_array)
            .ok_or("toolchain manifest needs arguments")?
            .iter()
            .map(|arg| {
                arg.as_str()
                    .map(str::to_owned)
                    .ok_or("toolchain arguments must be strings")
            })
            .collect::<Result<Vec<_>, _>>()?;
        for required in ["{object}", "{runtime}", "{output}", "{native-libs}"] {
            if arguments
                .iter()
                .filter(|arg| arg.as_str() == required)
                .count()
                != 1
            {
                return Err(format!(
                    "toolchain arguments require exactly one {required}"
                ));
            }
        }
        Ok(Self { linker, arguments })
    }

    fn arguments(
        &self,
        root: &Path,
        object: &Path,
        runtime: &Path,
        output: &Path,
        libraries: &str,
    ) -> Result<Vec<OsString>, String> {
        self.arguments
            .iter()
            .try_fold(Vec::new(), |mut result, argument| {
                let value: &OsStr = match argument.as_str() {
                    "{object}" => object.as_os_str(),
                    "{runtime}" => runtime.as_os_str(),
                    "{output}" => output.as_os_str(),
                    "{native-libs}" => {
                        result.extend(
                            libraries
                                .lines()
                                .filter(|line| !line.is_empty())
                                .map(OsString::from),
                        );
                        return Ok(result);
                    }
                    argument => {
                        if let Some((prefix, suffix)) = argument.split_once("{root}") {
                            if !suffix.starts_with('/') {
                                return Err(
                                    "toolchain root placeholder must precede a relative path"
                                        .into(),
                                );
                            }
                            let relative = Path::new(&suffix[1..]);
                            if relative
                                .components()
                                .any(|part| !matches!(part, Component::Normal(_)))
                            {
                                return Err("toolchain argument escapes root".into());
                            }
                            if prefix.contains('{') || prefix.contains('}') {
                                return Err("unknown toolchain placeholder".into());
                            }
                            let mut path = OsString::from(prefix);
                            path.push(root.join(relative));
                            result.push(path);
                            return Ok(result);
                        }
                        if argument.contains('{') || argument.contains('}') {
                            return Err(format!("unknown toolchain placeholder: {argument}"));
                        }
                        OsStr::new(argument)
                    }
                };
                result.push(value.to_owned());
                Ok(result)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn manifest(linker: &str, arguments: &str) -> String {
        format!(
            "version = 1\ntarget = \"test\"\nlinker = \"{linker}\"\narguments = [{arguments}]\n"
        )
    }
    const ARGUMENTS: &str = "\"{object}\", \"{runtime}\", \"-o\", \"{output}\", \"{native-libs}\", \"{root}/lib/start.o\"";
    #[test]
    fn preserves_paths_with_spaces_and_expands_native_libraries() {
        let plan = Plan::parse(&manifest("bin/ld", ARGUMENTS), "test").unwrap();
        let arguments = plan
            .arguments(
                Path::new("installation with spaces"),
                Path::new("source object"),
                Path::new("runtime archive"),
                Path::new("output app"),
                "-lSystem\n-lm",
            )
            .unwrap();
        assert_eq!(
            arguments,
            vec![
                OsString::from("source object"),
                "runtime archive".into(),
                "-o".into(),
                "output app".into(),
                "-lSystem".into(),
                "-lm".into(),
                Path::new("installation with spaces")
                    .join("lib/start.o")
                    .into_os_string()
            ]
        );
    }
    #[test]
    fn rejects_wrong_targets_escaping_linkers_and_incomplete_plans() {
        assert!(Plan::parse(&manifest("bin/ld", ARGUMENTS), "wrong").is_err());
        assert!(Plan::parse(&manifest("../ld", ARGUMENTS), "test").is_err());
        assert!(Plan::parse(&manifest("bin/ld", "\"{object}\""), "test").is_err());
        let plan = Plan::parse(
            &manifest("bin/ld", &format!("{ARGUMENTS}, \"{{root}}/../outside\"")),
            "test",
        )
        .unwrap();
        assert!(
            plan.arguments(
                Path::new("root"),
                Path::new("o"),
                Path::new("r"),
                Path::new("a"),
                ""
            )
            .is_err()
        );
    }
}
