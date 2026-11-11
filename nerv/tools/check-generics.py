import argparse
from pathlib import Path
import runpy
import subprocess


ROOT = Path(__file__).resolve().parent.parent
validate_ir = runpy.run_path(str(Path(__file__).with_name("check-sema.py")))["validate_ir"]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--stage0", required=True, type=Path)
    parser.add_argument("--compiler", required=True, type=Path)
    args = parser.parse_args()
    stage0 = args.stage0.resolve(strict=True)
    compiler = args.compiler.resolve(strict=True)
    cases = [
        (True, "fn identity<T>(value: T) -> T => value\nfn main() -> i64\n    let xs: []i64 = identity([])\n    nerv.arrays.len(xs)\n"),
        (True, "fn identity<T>(value: T) -> T => value\nfn main() -> i64 => identity(42)\n"),
        (True, "fn identity<T>(value: T) -> T => value\nfn main()\n    println(identity(1))\n    println(identity(true))\n    println(identity(\"hello\"))\n"),
        (True, "fn first<T>(values: []T) -> T => values[0]\nfn main() -> i64 => first([42])\n"),
        (True, "fn apply<T>(f: fn(T) -> T, value: T) -> T => f(value)\nfn add(n: i64) -> i64 => n + 1\nfn main() -> i64 => apply(add, 41)\n"),
        (True, "fn empty<T>() -> []T => []\nfn main() -> i64\n    let xs: []i64 = empty()\n    nerv.arrays.len(xs)\n"),
        (True, "fn repeat<T>(n: i64, value: T) -> T\n    if n == 0\n        return value\n    repeat(n - 1, value)\nfn main() -> i64 => repeat(3, 42)\n"),
        (True, "fn unused<T>(value: T) -> T => absent\nfn main() -> i64 => 0\n"),
        (True, "fn make<T>(value: T) -> fn() -> T\n    fn() -> T => value\nfn main() -> i64\n    let f = make(42)\n    f()\n"),
        (False, "fn choose<T>(a: T, b: T) -> T => a\nfn main() -> i64 => choose(1, true)\n"),
        (False, "fn unused<T>()\n    println(0)\nfn main()\n    unused()\n"),
        (False, "fn broken<T>(value: T) -> T => absent\nfn main() -> i64 => broken(1)\n"),
        (False, "fn f<T>(value: T) -> T => value\nfn f(value: i64) -> i64 => value\n"),
        (False, "fn f(value: i64) -> i64 => value\nfn f<T>(value: T) -> T => value\n"),
        (False, "fn grow<T>(value: T) -> i64 => grow([value])\nfn main() -> i64 => grow(0)\n"),
    ]
    directory = ROOT / "target/generic-cases"
    directory.mkdir(parents=True, exist_ok=True)
    for index, (accepted, text) in enumerate(cases):
        file = directory / f"case_{index}.nerv"
        file.write_text(text, encoding="utf-8", newline="")
        old = subprocess.run([str(stage0), str(file), "--check"], capture_output=True, text=True, encoding="utf-8", timeout=60)
        new = subprocess.run([str(compiler), "--ir", str(file)], capture_output=True, text=True, encoding="utf-8", timeout=60)
        expected = 0 if accepted else 1
        if old.returncode != expected or new.returncode != expected:
            raise RuntimeError(f"generic mismatch: {file}\nreference {old.returncode}: {old.stderr}\nnew {new.returncode}: {new.stderr}")
        if accepted:
            validate_ir(new.stdout, file.read_bytes())
        elif new.stdout:
            raise RuntimeError(f"partial generic IR: {file}")
    print(f"Matched stage 0 on {len(cases)} generic-function cases, including the specialization limit")


if __name__ == "__main__":
    main()
