import argparse
from pathlib import Path
import runpy
import subprocess


ROOT = Path(__file__).resolve().parent.parent
validate_ir = runpy.run_path(str(Path(__file__).with_name("check-sema.py")))["validate_ir"]
READ = "trait Read\n    fn read(self) -> i64\n"
VALUE = "struct Value implements Read\n    number: i64\n    fn read(self) -> i64 => self.number\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--stage0", type=Path, required=True)
    parser.add_argument("--compiler", type=Path, required=True)
    args = parser.parse_args()
    stage0 = args.stage0.resolve(strict=True)
    compiler = args.compiler.resolve(strict=True)
    cases = [
        (True, READ + VALUE + "struct Holder<T: Read>\n    value: T\nfn main() -> i64 => Holder { value: Value { number: 42 } }.value:read()\n"),
        (False, READ + "struct Holder<T: Read>\n    value: T\nfn main() -> i64 => Holder { value: 42 }.value\n"),
        (True, READ + "enum E\n    A\nimpl E for Read\n    fn read(self) -> i64 => 42\nfn fetch<T: Read>(value: T) -> i64 => value:read()\nfn main() -> i64 => fetch(A())\n"),
        (True, READ + "struct Value\n    number: i64\nstruct Holder<T: Read>\n    value: T\nfn take(value: Holder<Value>) -> i64 => value.value:read()\nimpl Value for Read\n    fn read(self) -> i64 => self.number\nfn main() -> i64 => take(Holder { value: Value { number: 42 } })\n"),
        (True, READ + VALUE + "fn fetch<T: Read>(value: T) -> i64 => value:read()\nfn main() -> i64 => fetch(Value { number: 42 })\n"),
        (True, VALUE + READ + "fn main() -> i64 => Value { number: 42 }:read()\n"),
        (True, READ + "struct Value\n    number: i64\nimpl Value for Read\n    fn read(self) -> i64 => self.number\nfn fetch<T: Read>(value: T) -> i64 => value:read()\nfn main() -> i64 => fetch(Value { number: 42 })\n"),
        (True, "struct Value\n    number: i64\nimpl Value\n    fn get(self) -> i64 => self.number\nfn main() -> i64 => Value { number: 42 }:get()\n"),
        (True, READ + "struct Box<T> implements Read\n    value: T\n    fn read(self) -> i64 => 42\nfn fetch<T: Read>(value: T) -> i64 => value:read()\nfn main() -> i64 => fetch(Box { value: true })\n"),
        (True, "trait Copy\n    fn copy(self) -> Copy\nstruct Value implements Copy\n    number: i64\n    fn copy(self) -> Value => Value { number: self.number }\nfn duplicate<T: Copy>(value: T) -> T => value:copy()\nfn main() -> i64 => duplicate(Value { number: 42 }).number\n"),
        (False, READ + "fn fetch<T: Read>(value: T) -> T => value\nfn main() -> i64 => fetch(42)\n"),
        (False, READ + "struct Value\n    number: i64\n    fn read(self) -> i64 => self.number\nfn fetch<T: Read>(value: T) -> i64 => value:read()\nfn main() -> i64 => fetch(Value { number: 42 })\n"),
        (False, READ + VALUE + "impl Value for Read\n    fn extra(self) -> i64 => 0\n"),
        (False, "struct Value\n    number: i64\nimpl Value for Missing\n    fn read(self) -> i64 => self.number\n"),
        (False, READ + READ),
    ]
    directory = ROOT / "target/trait-cases"
    directory.mkdir(parents=True, exist_ok=True)
    for index, (accepted, text) in enumerate(cases):
        file = directory / f"case_{index}.nerv"
        file.write_text(text, encoding="utf-8", newline="")
        old = subprocess.run([str(stage0), str(file), "--check"], capture_output=True, text=True, encoding="utf-8", timeout=45)
        new = subprocess.run([str(compiler), "--ir", str(file)], capture_output=True, text=True, encoding="utf-8", timeout=45)
        expected = 0 if accepted else 1
        if old.returncode != expected or new.returncode != expected:
            raise RuntimeError(f"trait mismatch: {file}\nreference {old.returncode}: {old.stderr}\nnew {new.returncode}: {new.stderr}")
        if accepted:
            validate_ir(new.stdout, file.read_bytes())
        elif new.stdout:
            raise RuntimeError(f"partial trait IR: {file}")
    strict = [READ + "struct Bad implements Read\n    n: i64\n", READ + "struct Bad implements Read\n    fn read(self) -> bool => true\n"]
    for index, text in enumerate(strict):
        file = directory / f"contract_{index}.nerv"
        file.write_text(text, encoding="utf-8", newline="")
        reference = subprocess.run([str(stage0), str(file), "--check"], capture_output=True, text=True, encoding="utf-8", timeout=45)
        if reference.returncode != 0:
            raise RuntimeError(f"strict-contract reference behavior changed: {file}: {reference.stderr}")
        result = subprocess.run([str(compiler), "--ir", str(file)], capture_output=True, text=True, encoding="utf-8", timeout=45)
        if result.returncode != 1 or result.stdout or "trait method" not in result.stderr:
            raise RuntimeError(f"trait contract was not rejected: {file}: {result}")
    print(f"Matched stage 0 on {len(cases)} trait cases; {len(strict)} additional strict contract rejections validated")


if __name__ == "__main__":
    main()
