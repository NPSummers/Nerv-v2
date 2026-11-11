import argparse
from pathlib import Path
import subprocess

import runpy

validate_ir = runpy.run_path(str(Path(__file__).with_name("check-sema.py")))["validate_ir"]


ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--stage0", type=Path, required=True)
    parser.add_argument("--compiler", type=Path, required=True)
    args = parser.parse_args()
    stage0 = args.stage0.resolve(strict=True)
    compiler = args.compiler.resolve(strict=True)
    cases = [
        (True, {"main.nerv": "import math\nfn main() -> i64 => math.add(20, 22)\n", "math.nerv": "fn add(a: i64, b: i64) -> i64 => a + b\n"}),
        (True, {"main.nerv": "import math\nfn main() -> i64 => add(20, 22)\n", "math.nerv": "fn add(a: i64, b: i64) -> i64 => a + b\n"}),
        (True, {"main.nerv": "import left\nimport right\nfn main() -> i64 => left.value() + right.value()\n", "left.nerv": "fn value() -> i64 => 20\n", "right.nerv": "fn value() -> i64 => 22\n"}),
        (False, {"main.nerv": "import left\nimport right\nfn main() -> i64 => value()\n", "left.nerv": "fn value() -> i64 => 20\n", "right.nerv": "fn value() -> i64 => 22\n"}),
        (True, {"main.nerv": "import left\nfn value() -> i64 => 42\nfn main() -> i64 => value()\n", "left.nerv": "fn value() -> i64 => 20\n"}),
        (True, {"main.nerv": "import left\nimport right\nfn main() -> i64 => left.get() + right.get()\n", "left.nerv": "import shared\nfn get() -> i64 => shared.value()\n", "right.nerv": "import shared\nfn get() -> i64 => shared.value()\n", "shared.nerv": "fn value() -> i64 => 21\n"}),
        (True, {"main.nerv": "import lib.math\nfn main() -> i64 => math.value()\n", "lib/math.nerv": "module math\nfn value() -> i64 => 42\n"}),
        (True, {"main.nerv": "import lib.math\nfn main() -> i64 => lib.math.value()\n", "lib/math.nerv": "fn value() -> i64 => 42\n"}),
        (True, {"main.nerv": "import left\nimport right\nfn main() -> i64 => left.Value { n: 20 }:get() + right.Value { n: 22 }.n\n", "left.nerv": "struct Value\n    n: i64\n    fn get(self) -> i64 => self.n\n", "right.nerv": "struct Value\n    n: i64\n"}),
        (True, {"main.nerv": "import choices\nfn main() -> i64\n    let x = choices.Value.Number(42)\n    match x\n        choices.Value.Number(n) => n\n        choices.Value.Empty => 0\n", "choices.nerv": "enum Value\n    Number(i64)\n    Empty\n"}),
        (True, {"main.nerv": "import left\nimport right\nfn main() -> i64 => left.get()\n", "left.nerv": "fn get() -> i64 => private_value()\nfn private_value() -> i64 => 42\n", "right.nerv": "fn private_value() -> i64 => 0\n"}),
        (False, {"main.nerv": "import lib\nfn main() -> i64 => lib.bad()\n", "lib.nerv": "fn bad() -> i64 => missing\n"}),
        (False, {"main.nerv": "import absent\nfn main() -> i64 => 0\n"}),
        (False, {"main.nerv": "import main\nfn main() -> i64 => 0\n"}),
        (False, {"main.nerv": "import child\nfn main() -> i64 => 0\n", "child.nerv": "import main\n"}),
        (False, {"main.nerv": "import bad\nfn main() -> i64 => 0\n", "bad.nerv": "fn broken( => 0\n"}),
    ]
    qualified_match = dict(cases[9][1])
    cases[9][1]["main.nerv"] = cases[9][1]["main.nerv"].replace("choices.Value.Number(n)", "Number(n)").replace("choices.Value.Empty =>", "Empty =>")
    cases += [
        (True, {"main.nerv": "import lib\nfn main() -> i64 => lib.fetch(lib.Value { n: 42 })\n", "lib.nerv": "trait Read\n    fn read(self) -> i64\nstruct Value implements Read\n    n: i64\n    fn read(self) -> i64 => self.n\nfn fetch<T: Read>(value: T) -> i64 => value:read()\n"}),
        (False, {"main.nerv": "import left\nimport right\nfn main() -> i64 => left.fetch(right.Value { n: 42 })\n", "left.nerv": "trait Read\n    fn read(self) -> i64\nfn fetch<T: Read>(value: T) -> i64 => value:read()\n", "right.nerv": "trait Read\n    fn read(self) -> i64\nstruct Value implements Read\n    n: i64\n    fn read(self) -> i64 => self.n\n"}),
        (True, {"main.nerv": "import left\nimport right\nfn a() -> left.Box<i64> => left.Box { value: 20 }\nfn b() -> right.Box<i64> => right.Box { value: 22 }\nfn main() -> i64 => a().value + b().value\n", "left.nerv": "struct Box<T>\n    value: T\n", "right.nerv": "struct Box<T>\n    value: T\n"}),
        (True, {"main.nerv": "import util\nfn main() -> i64 => util.unwrap(util.Box { value: 42 })\n", "util.nerv": "struct Box<T>\n    value: T\nfn unwrap<T>(box: Box<T>) -> T => box.value\n"}),
        (False, {"main.nerv": "import left\nimport right\nfn main() -> i64 => identity(42)\n", "left.nerv": "fn identity<T>(value: T) -> T => value\n", "right.nerv": "fn identity(value: i64) -> i64 => value\n"}),
        (True, {"main.nerv": "import math\nfn main() -> i64 => math.identity(42)\n", "math.nerv": "fn identity<T>(value: T) -> T => value\n"}),
        (True, {"main.nerv": "import left\nimport right\nfn main() -> i64 => left.identity(20) + right.identity(22)\n", "left.nerv": "fn identity<T>(value: T) -> T => value\n", "right.nerv": "fn identity<T>(value: T) -> T => value\n"}),
        (True, {"main.nerv": "import other\nfn identity<T>(value: T) -> T => value\nfn main() -> i64 => identity(42)\n", "other.nerv": "fn identity(value: bool) -> bool => value\n"}),
        (False, {"main.nerv": "import other\nfn main() -> i64 => other.broken(1)\n", "other.nerv": "fn broken<T>(value: T) -> T => missing\n"}),
    ]
    cases.append((True, qualified_match))
    fixtures = ROOT / "target/module-cases"
    for index, (accepted, files) in enumerate(cases):
        directory = fixtures / f"case_{index}"
        for name, text in files.items():
            file = directory / name
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text(text, encoding="utf-8", newline="")
        entry = directory / "main.nerv"
        old = subprocess.run([str(stage0), str(entry), "--check"], capture_output=True, text=True, encoding="utf-8", timeout=30)
        new = subprocess.run([str(compiler), "--ir", str(entry)], capture_output=True, text=True, encoding="utf-8", timeout=30)
        expected = 0 if accepted else 1
        reference_expected = 1 if index == len(cases) - 1 else expected
        if old.returncode != reference_expected or new.returncode != expected:
            raise RuntimeError(f"module mismatch: case {index}\nreference {old.returncode}: {old.stderr}\nnew {new.returncode}: {new.stderr}")
        if accepted:
            validate_ir(new.stdout, entry.read_bytes())
        elif new.stdout:
            raise RuntimeError(f"partial IR on module failure: {index}")
        if index == 11 and "lib.nerv:" not in new.stderr:
            raise RuntimeError(f"wrong diagnostic source: {new.stderr}")
    print(f"Matched stage 0 on {len(cases) - 1} multi-file cases plus one qualified-pattern extension; checked namespaces, deduplication, cycles, and source diagnostics")


if __name__ == "__main__":
    main()
