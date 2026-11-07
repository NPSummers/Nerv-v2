use std::{env, fs, path::PathBuf, process::Command};

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf();
    println!("cargo:rerun-if-env-changed=LLVM_SYS_231_PREFIX");
    println!("cargo:rerun-if-env-changed=LLVM_CONFIG_PATH");
    println!(
        "cargo:rustc-env=NERV_CODEGEN_TARGET={}",
        env::var("TARGET")
            .unwrap()
            .replace("windows-gnullvm", "windows-gnu")
    );
    let llvm = env::var_os("LLVM_SYS_231_PREFIX")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("llvm"));
    println!(
        "cargo:rustc-link-search=native={}",
        llvm.join("lib").display()
    );
    if env::var("CARGO_CFG_TARGET_OS").unwrap() != "windows" {
        if llvm.join("lib/libLLVM-23.dylib").is_file() {
            println!("cargo:rustc-link-lib=dylib=LLVM-23");
            return;
        }
        if llvm.join("lib/libLLVM-23.so").is_file() {
            println!("cargo:rustc-link-lib=dylib=LLVM-23");
            return;
        }
        let config = env::var_os("LLVM_CONFIG_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| llvm.join("bin/llvm-config"));
        let output = Command::new(config)
            .args(["--link-shared", "--libs"])
            .output()
            .expect("could not run llvm-config");
        assert!(
            output.status.success(),
            "llvm-config failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        for library in String::from_utf8(output.stdout).unwrap().split_whitespace() {
            let name = library
                .strip_prefix("-l")
                .expect("llvm-config must report -l libraries");
            println!("cargo:rustc-link-lib=dylib={name}");
        }
        return;
    }
    let dll = llvm.join("bin/LLVM-C.dll");
    println!("cargo:rerun-if-changed={}", dll.display());
    println!("cargo:rustc-link-lib=dylib=LLVM-C");

    let profile = PathBuf::from(env::var("OUT_DIR").unwrap())
        .ancestors()
        .nth(3)
        .unwrap()
        .to_path_buf();
    for directory in [profile.clone(), profile.join("deps")] {
        fs::create_dir_all(&directory).unwrap();
        fs::copy(&dll, directory.join("LLVM-C.dll")).unwrap();
    }
}
