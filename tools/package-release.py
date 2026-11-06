import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile
import zipfile


ROOT = Path(__file__).resolve().parent.parent


def run(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def licenses(destination):
    metadata = json.loads(run("cargo", "metadata", "--locked", "--offline", "--format-version", "1"))
    for package in metadata["packages"]:
        source = Path(package["manifest_path"]).parent
        if package["source"] is None:
            continue
        target = destination / f"{package['name']}-{package['version']}"
        for path in source.iterdir():
            if path.is_file() and (path.name.upper().startswith("LICENSE") or path.name.upper().startswith("COPYING") or path.name.upper().startswith("NOTICE")):
                target.mkdir(parents=True, exist_ok=True)
                shutil.copy2(path, target / path.name)
    sysroot = Path(run("rustc", "--print", "sysroot"))
    rust_licenses = sysroot / "share/doc/rust/licenses"
    if rust_licenses.is_dir():
        shutil.copytree(rust_licenses, destination / "rust")
    for notice in (ROOT / "licenses").iterdir():
        if notice.is_file():
            shutil.copy2(notice, destination / notice.name)


def mac_libraries(binary, llvm, destination):
    library = llvm / "lib/libLLVM-23.dylib"
    shutil.copy2(library, destination / library.name)
    queue = [binary, destination / library.name]
    visited = set()
    while queue:
        item = queue.pop()
        if item in visited:
            continue
        visited.add(item)
        imports = run("otool", "-L", str(item)).splitlines()[1:]
        for line in imports:
            dependency = line.strip().split(" (", 1)[0]
            if dependency.startswith(("/usr/lib/", "/System/Library/", "@executable_path/")):
                continue
            name = Path(dependency).name
            target = destination / name
            if not target.exists():
                origin = Path(dependency)
                if not origin.is_file():
                    raise RuntimeError(f"cannot bundle dependency {dependency}")
                shutil.copy2(origin, target)
                queue.append(target)
            subprocess.run(["install_name_tool", "-change", dependency, f"@executable_path/{name}", str(item)], check=True)
        subprocess.run(["codesign", "--force", "--sign", "-", str(item)], check=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--llvm", type=Path, default=ROOT / "llvm")
    parser.add_argument("--output", type=Path, default=ROOT / "dist")
    args = parser.parse_args()
    metadata = json.loads(run("cargo", "metadata", "--locked", "--offline", "--no-deps", "--format-version", "1"))
    version = next(p["version"] for p in metadata["packages"] if p["name"] == "nerv")
    if version != args.version:
        raise RuntimeError(f"package version {version} does not match {args.version}")
    if args.target not in ("x86_64-pc-windows-msvc", "aarch64-apple-darwin"):
        raise RuntimeError("only Windows x64 and Apple Silicon release bundles are supported")
    windows = args.target.endswith("windows-msvc")
    if windows != (platform.system() == "Windows"):
        raise RuntimeError("package on the native host")
    name = f"nerv-{args.version}-{args.target}"
    args.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="nerv-package-") as temporary:
        bundle = Path(temporary) / name
        bundle.mkdir()
        binary_name = "nerv.exe" if windows else "nerv"
        binary = bundle / binary_name
        shutil.copy2(ROOT / "target/release" / binary_name, binary)
        if windows:
            shutil.copy2(args.llvm / "bin/LLVM-C.dll", bundle / "LLVM-C.dll")
        else:
            mac_libraries(binary, args.llvm, bundle)
        shutil.copytree(ROOT / "docs", bundle / "docs")
        shutil.copy2(ROOT / "LICENSE", bundle / "LICENSE")
        shutil.copy2(ROOT / "docs/installation.md", bundle / "README.md")
        (bundle / "licenses").mkdir()
        licenses(bundle / "licenses")
        (bundle / "VERSION").write_text(f"{args.version}\n{args.target}\n{run('git', 'rev-parse', 'HEAD')}\n", encoding="utf-8")
        environment = dict(os.environ)
        for key in ("LLVM_SYS_231_PREFIX", "LLVM_CONFIG_PATH", "DYLD_LIBRARY_PATH", "DYLD_FALLBACK_LIBRARY_PATH", "LD_LIBRARY_PATH"):
            environment.pop(key, None)
        subprocess.run([str(binary), "--version"], cwd=temporary, env=environment, check=True)
        subprocess.run([str(binary), "new", "smoke"], cwd=temporary, env=environment, check=True)
        subprocess.run([str(binary), "check", "--locked"], cwd=Path(temporary) / "smoke", env=environment, check=True)
        subprocess.run([str(binary), "run", "--locked"], cwd=Path(temporary) / "smoke", env=environment, check=True)
        archive = args.output / f"{name}.zip"
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9, strict_timestamps=False) as output:
            for path in sorted(bundle.rglob("*")):
                if path.is_file():
                    output.write(path, path.relative_to(bundle.parent))
    checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_suffix(".zip.sha256").write_text(f"{checksum}  {archive.name}\n", encoding="ascii")
    print(archive)


if __name__ == "__main__":
    main()
