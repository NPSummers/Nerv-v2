import argparse
import json
from pathlib import Path
import runpy
import subprocess


ROOT = Path(__file__).resolve().parent.parent
validate_ir = runpy.run_path(str(Path(__file__).with_name("check-sema.py")))["validate_ir"]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--compiler", required=True, type=Path)
    args = parser.parse_args()
    compiler = args.compiler.resolve(strict=True)
    source = ROOT / "src/main.nerv"
    result = subprocess.run([str(compiler), "--ir", str(source)], cwd=ROOT, capture_output=True, text=True, encoding="utf-8", timeout=120)
    if result.returncode:
        raise RuntimeError(result.stderr)
    validate_ir(result.stdout, source.read_bytes())
    rows = [json.loads(line) for line in result.stdout.splitlines()]
    sources = sum(row[0] == "source" for row in rows)
    nodes = sum(row[0] == "node" for row in rows)
    functions = len(rows[-1][2])
    if rows[-1][1] < 0 or sources < 2 or functions == 0:
        raise RuntimeError("self check omitted its entry, imports, or checked functions")
    for library in sorted((ROOT / "stdlib").glob("*.nerv")):
        checked = subprocess.run([str(compiler), "--ir", str(library)], cwd=ROOT, capture_output=True, text=True, encoding="utf-8", timeout=120)
        if checked.returncode:
            raise RuntimeError(f"standard source failed: {library}\n{checked.stderr}")
        validate_ir(checked.stdout, library.read_bytes())
    print(f"Own-source semantic/IR check passed: {sources} sources, {functions} functions, {nodes} checked operations. No machine code generated.")
    print("All four bundled standard-source modules also passed semantic/IR validation.")


if __name__ == "__main__":
    main()
