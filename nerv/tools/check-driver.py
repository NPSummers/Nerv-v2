import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time


ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--compiler", type=Path, required=True)
    args = parser.parse_args()
    compiler = args.compiler.resolve(strict=True)
    base = ROOT / "target/driver-cases"
    base.mkdir(exist_ok=True)
    package = base / f"app_{time.time_ns()}"
    def run(*arguments, cwd=ROOT, env=None):
        return subprocess.run([str(compiler), *map(str, arguments)], cwd=cwd, env=env, capture_output=True, text=True, encoding="utf-8", timeout=120)
    created = run("new", package)
    if created.returncode:
        raise RuntimeError(created.stderr)
    before = (package / "src/main.nerv").read_bytes()
    if run("new", package).returncode != 1 or (package / "src/main.nerv").read_bytes() != before:
        raise RuntimeError("new overwrote an existing project")
    source = package / "src/main.nerv"
    source.write_text('import nerv.std\nfn main() -> i64\n    println(arguments()[1])\n    7\n', encoding="utf-8")
    output = package / ("app.exe" if os.name == "nt" else "app")
    if run("build", "--locked", "-o", output, cwd=package).returncode:
        raise RuntimeError(run("build", "--locked", "-o", output, cwd=package).stderr)
    object_file = Path(str(output) + ".nerv-object")
    stamp = Path(str(output) + ".nerv-cache")
    lines = stamp.read_text(encoding="utf-8").splitlines()
    if lines[1] != hashlib.sha256(object_file.read_bytes()).hexdigest():
        raise RuntimeError("native SHA-256 does not match hashlib")
    first = object_file.stat().st_mtime_ns
    if run("build", "--locked", "-o", output, cwd=package).returncode or object_file.stat().st_mtime_ns != first:
        raise RuntimeError("cache did not reuse the object")
    source.write_text(source.read_text(encoding="utf-8").replace("    7", "    8"), encoding="utf-8")
    if run("build", "--locked", "-o", output, cwd=package).returncode or stamp.read_text(encoding="utf-8").splitlines()[0] == lines[0]:
        raise RuntimeError("source edit did not invalidate cache")
    object_file.write_bytes(b"corrupted cache")
    if run("build", "--locked", "-o", output, cwd=package).returncode or object_file.read_bytes() == b"corrupted cache":
        raise RuntimeError("corrupt cache was accepted")
    old = object_file.stat().st_mtime_ns
    if run("build", "--locked", "--no-cache", "-o", output, cwd=package).returncode or object_file.stat().st_mtime_ns == old:
        raise RuntimeError("--no-cache reused an object")
    executed = run("run", "--locked", "-o", output, "--", "forwarded", cwd=package)
    if executed.returncode != 8 or executed.stdout != "forwarded\n":
        raise RuntimeError(f"argument/status forwarding failed: {executed}")
    config_path = Path(os.environ.get("NERV_NATIVE_CONFIG", ROOT / "target/native.json"))
    config = json.loads(config_path.read_text(encoding="utf-8"))
    config["runtime"] = str(base / "absent-runtime")
    bad = base / "bad.json"
    bad.write_text(json.dumps(config), encoding="utf-8")
    env = dict(os.environ, NERV_NATIVE_CONFIG=str(bad))
    executable_before = output.read_bytes()
    failed = run("build", "--locked", "-o", output, cwd=package, env=env)
    if failed.returncode != 1 or output.read_bytes() != executable_before:
        raise RuntimeError("failed link changed the previous executable")
    commands = [[str(compiler), "build", "--locked", "-o", str(output)]] * 2
    children = [subprocess.Popen(command, cwd=package, stdout=subprocess.PIPE, stderr=subprocess.PIPE) for command in commands]
    for child in children:
        stdout, stderr = child.communicate(timeout=120)
        if child.returncode:
            raise RuntimeError(f"concurrent build failed: {stderr}")
    if run("fmt", cwd=package).returncode or run("fmt", "--check", cwd=package).returncode:
        raise RuntimeError("formatter rejected generated project")
    print("Driver passed: new project, overwrite protection, cache hashes/reuse/invalidation/corruption, --no-cache, concurrent builds, arguments/status, failed-link preservation")


if __name__ == "__main__":
    main()
