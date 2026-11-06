use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    println!("cargo:rerun-if-changed=../nerv-runtime/src");
    println!("cargo:rerun-if-changed=../nerv-runtime/Cargo.toml");
    println!("cargo:rerun-if-changed=../../Cargo.lock");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    let target = env::var("TARGET").unwrap();
    println!("cargo:rustc-env=NERV_BUILD_TARGET={target}");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let runtime_target = out.join("native-runtime");
    let mut command = Command::new(env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command
        .args([
            "rustc",
            "--locked",
            "--offline",
            "--release",
            "--lib",
            "-p",
            "nerv-runtime",
            "--manifest-path",
        ])
        .arg(root.join("Cargo.toml"))
        .args(["--target", &target, "--target-dir"])
        .arg(&runtime_target)
        .args(["--", "-Cpanic=abort", "--print", "native-static-libs"])
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .env(
            "RUSTFLAGS",
            if target.contains("windows-msvc") {
                "-Ctarget-feature=+crt-static"
            } else {
                ""
            },
        );
    let output = command
        .output()
        .expect("could not build the standalone runtime with Cargo");
    assert!(
        output.status.success(),
        "standalone runtime build failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    let libs = stderr
        .lines()
        .find_map(|line| {
            line.split_once("native-static-libs: ")
                .map(|(_, libs)| libs)
        })
        .expect("rustc did not report standalone native link libraries");
    fs::write(
        out.join("runtime-link-libs.txt"),
        libs.split_whitespace().collect::<Vec<_>>().join("\n"),
    )
    .unwrap();
    let archive = if target.contains("windows-msvc") {
        "nerv_runtime.lib"
    } else {
        "libnerv_runtime.a"
    };
    fs::copy(
        runtime_target.join(&target).join("release").join(archive),
        out.join("runtime.archive"),
    )
    .unwrap();
}
