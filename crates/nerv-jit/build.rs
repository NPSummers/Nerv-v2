use std::{env, fs, path::PathBuf};

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf();
    let llvm = root.join("clang");
    let dll = llvm.join("bin").join("LLVM-C.dll");

    println!("cargo:rerun-if-changed={}", dll.display());
    println!(
        "cargo:rustc-link-search=native={}",
        llvm.join("lib").display()
    );
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
