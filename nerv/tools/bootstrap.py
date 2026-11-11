import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import subprocess
import tomllib


ROOT = Path(__file__).resolve().parent.parent


def run(arguments, env):
    subprocess.run([str(value) for value in arguments], cwd=ROOT, env=env, check=True)


def snapshot():
    digest = hashlib.sha256()
    files = [ROOT / "Nerv.toml", ROOT / "Nerv.lock"]
    for directory in ["src", "stdlib", "tests", "bootstrap"]:
        files.extend(file for file in (ROOT / directory).rglob("*") if file.suffix in {".nerv", ".rs", ".toml", ".lock"})
    for file in sorted(set(files)):
        digest.update(file.relative_to(ROOT).as_posix().encode())
        digest.update(b"\0")
        digest.update(file.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--llvm", required=True, type=Path)
    parser.add_argument("--toolchain", required=True, type=Path)
    parser.add_argument("--cc", type=Path)
    parser.add_argument("--offline", action="store_true")
    args = parser.parse_args()
    llvm = args.llvm.resolve(strict=True)
    toolchain = args.toolchain.resolve(strict=True)
    plan = tomllib.loads((toolchain / "Nerv-toolchain.toml").read_text(encoding="utf-8"))
    target = plan["target"]
    env = os.environ.copy()
    env["LLVM_SYS_231_PREFIX"] = str(llvm)
    if args.cc:
        cc = args.cc.resolve(strict=True)
        env["CARGO_TARGET_" + target.upper().replace("-", "_") + "_LINKER"] = str(cc)
        env["CC_" + target.replace("-", "_")] = str(cc)
        archive_tool = cc.parent / ("llvm-ar.exe" if os.name == "nt" else "llvm-ar")
        if archive_tool.is_file():
            env["AR_" + target.replace("-", "_")] = str(archive_tool)
    if platform.system() == "Darwin":
        sdk = subprocess.check_output(["xcrun", "--show-sdk-path"], text=True).strip()
        env["SDKROOT"] = sdk
        flags_key = "CFLAGS_" + target.replace("-", "_")
        env[flags_key] = (env.get(flags_key, "") + " -isysroot " + shlex.quote(sdk)).strip()
    source_snapshot = snapshot()
    command = ["cargo", "build", "--manifest-path", ROOT / "bootstrap/Cargo.toml", "--release", "--locked", "-p", "nerv", "--target", target, "--target-dir", ROOT / "target/bootstrap"]
    if args.offline:
        command.append("--offline")
    run(command, env)
    suffix = ".exe" if os.name == "nt" else ""
    stage0 = ROOT / "target/bootstrap" / target / "release" / ("nerv" + suffix)
    library_name = "LLVM-C.dll" if os.name == "nt" else ("libLLVM-23.dylib" if platform.system() == "Darwin" else "libLLVM-23.so")
    library = llvm / ("bin" if os.name == "nt" else "lib") / library_name
    if os.name == "nt":
        unwind = args.cc.parent / "libunwind.dll" if args.cc else llvm / "bin/libunwind.dll"
        if unwind.is_file():
            shutil.copy2(unwind, stage0.parent / "libunwind.dll")
    archives = sorted((ROOT / "target/bootstrap" / target / "release/build").glob("nerv-*/out/runtime.archive"), key=lambda value: value.stat().st_mtime)
    runtime = archives[-1]
    libraries = runtime.with_name("runtime-link-libs.txt").read_text(encoding="utf-8").splitlines()
    config = {"linker": str(toolchain / plan["linker"]), "arguments": plan["arguments"], "root": str(toolchain), "runtime": str(runtime), "native_libraries": libraries}
    config_file = ROOT / "target/native.json"
    config_file.write_text(json.dumps(config, indent=2) + "\n", encoding="utf-8")
    env["NERV_TOOLCHAIN"] = str(toolchain)
    env["NERV_LLVM"] = str(library)
    env["NERV_NATIVE_CONFIG"] = str(config_file)
    env["NERV_STDLIB"] = str(ROOT / "stdlib")
    run([stage0, "fmt", "--check"], env)
    run([stage0, "check", "--locked"], env)
    run([stage0, "test", "--locked"], env)
    run([stage0, "build", "--release", "--locked"], env)
    stage1 = ROOT / "target/nerv/release" / ("nerv_bootstrap" + suffix)
    shutil.copy2(config_file, stage1.parent / "native.json")
    generations = [stage1]
    for number in [2, 3]:
        output = ROOT / "target" / f"stage{number}{suffix}"
        run([generations[-1], "build", "--locked", "--release", "-o", output], env)
        generations.append(output)
    objects = []
    for number, compiler in enumerate(generations[1:], 2):
        output = ROOT / "target" / f"stage{number}-proof.o"
        run([compiler, "build", "--locked", "--release", "--emit-object", "-o", output], env)
        objects.append(hashlib.sha256(output.read_bytes()).hexdigest())
    if objects[0] != objects[1]:
        raise RuntimeError("stage 2 and stage 3 objects differ")
    for compiler in generations:
        run([compiler, "fmt", "--check"], env)
        run([compiler, "test", "--locked", "--release"], env)
        run(["python", ROOT / "tools/check-self.py", "--compiler", compiler], env)
        for name in ["projects", "traits", "nominals", "runtime", "sema", "generics", "modules", "format", "codegen"]:
            run(["python", ROOT / f"tools/check-{name}.py", "--stage0", stage0, "--compiler", compiler], env)
        run(["python", ROOT / "tools/check-driver.py", "--compiler", compiler], env)
        run([compiler, "--help"], env)
        run([compiler, "--version"], env)
        if args.cc:
            run(["python", ROOT / "tools/check-ffi.py", "--compiler", compiler, "--cc", args.cc.resolve()], env)
        run(["python", ROOT / "tools/check-parser.py", "--stage0", stage0, "--parser", compiler], env)
        run(["python", ROOT / "tools/check-lexer.py", "--stage0", stage0, "--lexer", compiler], env)
    if snapshot() != source_snapshot:
        raise RuntimeError("source changed during bootstrap verification")
    unit_tests = sum(len(re.findall(r"^test ", file.read_text(encoding="utf-8"), re.M)) for file in (ROOT / "tests").glob("*.nerv"))
    proof = {"source_sha256": source_snapshot, "unit_test_count": unit_tests, "target": target, "stage2_stage3_object_sha256": objects[0], "compilers": [str(value) for value in generations], "llvm": str(library)}
    (ROOT / "target/bootstrap-proof.json").write_text(json.dumps(proof, indent=2) + "\n", encoding="utf-8")
    print("Bootstrap verified: three compiler generations, identical stage 2/3 objects, tests passed")


if __name__ == "__main__":
    main()
