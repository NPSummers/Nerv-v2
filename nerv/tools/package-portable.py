import argparse
import json
import importlib.util
import platform
import sys
import time
import tomllib
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import zipfile


ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--compiler", required=True, type=Path)
    parser.add_argument("--llvm-library", required=True, type=Path)
    parser.add_argument("--config", type=Path, default=ROOT / "target/native.json")
    parser.add_argument("--reference", type=Path, default=Path(os.environ.get("NERV_REFERENCE", str(ROOT.parent / "Nerv-v2"))))
    parser.add_argument("--output", type=Path, default=ROOT / "target/portable")
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    compiler = args.compiler.resolve(strict=True)
    reference = args.reference.resolve(strict=True)
    sys.path.insert(0, str(reference / "tools"))
    spec = importlib.util.spec_from_file_location("nerv_distribution", reference / "tools/package-release.py")
    package = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(package)
    config = json.loads(args.config.read_text(encoding="utf-8"))
    shutil.copy2(compiler, output / ("nerv.exe" if os.name == "nt" else "nerv"))
    shutil.copy2(args.llvm_library.resolve(strict=True), output / args.llvm_library.name)
    unwind = args.llvm_library.parent / "libunwind.dll"
    if os.name == "nt" and not unwind.is_file():
        unwind = ROOT.parent / "Nerv-v2/dist/published-1.0.0/extracted/nerv-1.0.0-windows-x64/libunwind.dll"
    if unwind.is_file():
        shutil.copy2(unwind, output / unwind.name)
    root = Path(config["root"])
    shutil.copytree(root, output / "toolchain", dirs_exist_ok=True)
    shutil.copytree(ROOT / "stdlib", output / "stdlib", dirs_exist_ok=True)
    shutil.copy2(config["runtime"], output / "runtime.archive")
    config["linker"] = "toolchain/" + Path(config["linker"]).relative_to(root).as_posix()
    config["root"] = "toolchain"
    config["runtime"] = "runtime.archive"
    (output / "native.json").write_text(json.dumps(config, indent=2) + "\n", encoding="utf-8")
    shutil.copy2(ROOT / "LICENSE", output / "LICENSE")
    shutil.copy2(ROOT / "README.md", output / "README.md")
    version = tomllib.loads((ROOT / "Nerv.toml").read_text(encoding="utf-8"))["package"]["version"]
    (output / "VERSION").write_text(version + "\nSelf-hosted Nerv compiler\n", encoding="utf-8")
    if platform.system() == "Darwin":
        shutil.copy2(reference / "tools/installers/uninstall-macos.sh", output / "uninstall-macos.sh")
        (output / "uninstall-macos.sh").chmod(0o755)
    notices = output / "licenses"
    notices.mkdir(exist_ok=True)
    staging = ROOT / "target" / f"license-stage-{time.time_ns()}"
    staging.mkdir()
    try:
        package.licenses(staging)
        shutil.copytree(staging, notices, dirs_exist_ok=True)
    finally:
        if staging.resolve().parent != (ROOT / "target").resolve() or not staging.name.startswith("license-stage-"):
            raise RuntimeError("license staging path escaped the build directory")
        shutil.rmtree(staging)
    if platform.system() == "Darwin":
        package.mac_libraries(output / "nerv", args.llvm_library.resolve().parent.parent, output, original=compiler, extra=[(output / config["linker"], root / Path(config["linker"]).relative_to("toolchain"))])
    if platform.system() == "Linux":
        package.elf_libraries(output / "nerv", args.llvm_library.resolve().parent.parent, output, compiler, output / config["linker"])
    env = {key: value for key, value in os.environ.items() if not key.startswith("NERV_") and key not in {"LD_LIBRARY_PATH", "DYLD_LIBRARY_PATH", "DYLD_FALLBACK_LIBRARY_PATH", "LLVM_SYS_231_PREFIX", "LLVM_CONFIG_PATH", "LIB", "LIBPATH", "INCLUDE", "SDKROOT", "DEVELOPER_DIR", "CARGO_HOME", "RUSTUP_HOME"}}
    env["PATH"] = os.environ.get("SystemRoot", "C:/Windows") + "/System32" if os.name == "nt" else "/usr/bin:/bin"
    fixture = ROOT / "target/portable-smoke"
    fixture.mkdir(exist_ok=True)
    source = fixture / "hello.nerv"
    source.write_text('import nerv.std\nfn main() -> i64\n    println(integer_text(42))\n    0\n', encoding="utf-8")
    executable = fixture / ("hello.exe" if os.name == "nt" else "hello")
    driver = output / ("nerv.exe" if os.name == "nt" else "nerv")
    subprocess.run([str(driver), "build", str(source), "--release", "-o", str(executable)], cwd=fixture, env=env, check=True)
    result = subprocess.run([str(executable)], cwd=fixture, env=env, check=True, capture_output=True, text=True)
    if result.stdout != "42\n":
        raise RuntimeError("portable smoke output mismatch")
    archive = output.with_suffix(".zip" if os.name == "nt" else ".tar.gz")
    if os.name == "nt":
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED, strict_timestamps=False) as bundle:
            for file in sorted(output.rglob("*")):
                if file.is_file():
                    bundle.write(file, Path(output.name) / file.relative_to(output))
    else:
        with tarfile.open(archive, "w:gz") as bundle:
            bundle.add(output, arcname=output.name)
    print(f"Portable verified and packaged: {archive}")


if __name__ == "__main__":
    main()
