use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    let source = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("../nerv-runtime/src/lib.rs");
    println!("cargo:rerun-if-changed=../nerv-runtime/src");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    let target = env::var("TARGET").unwrap();
    println!("cargo:rustc-env=NERV_BUILD_TARGET={target}");
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    // The runtime has no third-party dependencies. Embed its target-native
    // static archive, so the installed compiler needs no Rust/Cargo at build time.
    let mut command = Command::new(env::var_os("RUSTC").unwrap());
    command
        .arg(&source)
        .args([
            "--crate-name",
            "nerv_runtime",
            "--crate-type",
            "staticlib",
            "--edition=2024",
            "--target",
            &target,
            "-Copt-level=2",
            "-Cdebuginfo=0",
            "-Cpanic=abort",
            "--print",
            "native-static-libs",
            "--out-dir",
        ])
        .arg(&out);
    if target.contains("windows-msvc") {
        command.arg("-Ctarget-feature=+crt-static");
    }
    let output = command
        .output()
        .expect("could not invoke rustc to build standalone runtime");
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
        .expect("rustc did not report the standalone runtime's native link libraries");
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
    fs::rename(out.join(archive), out.join("runtime.archive")).unwrap();
}
