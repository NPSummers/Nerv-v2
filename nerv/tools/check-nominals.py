import argparse
from pathlib import Path
import runpy
import subprocess


ROOT = Path(__file__).resolve().parent.parent
validate_ir = runpy.run_path(str(Path(__file__).with_name("check-sema.py")))["validate_ir"]
BOX = "struct Box<T>\n    value: T\n    fn get(self) -> T => self.value\n"
OPTION = "enum Option<T>\n    Some(T)\n    None\n"
RESULT = "enum Result<T, E>\n    Ok(T)\n    Err(E)\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--stage0", type=Path, required=True)
    parser.add_argument("--compiler", type=Path, required=True)
    args = parser.parse_args()
    stage0 = args.stage0.resolve(strict=True)
    compiler = args.compiler.resolve(strict=True)
    cases = [
        (True, "struct Box<T>\n    value: T\n    fn callback(self) -> fn() -> T => fn() -> T => self.value\nfn main() -> i64\n    let f = Box { value: 42 }:callback()\n    f()\n"),
        (True, RESULT + "fn a() -> Result<i64, string> => Ok(1)\nfn b() -> Result<bool, string> => Ok(true)\nfn c() -> Result<i64, string> => Err(\"bad\")\n"),
        (True, OPTION + "fn a() -> Option<i64> => None()\nfn b() -> Option<bool> => Some(true)\nfn c() -> Option<string> => Some(\"text\")\n"),
        (True, "import nerv.core\nfn main() -> i64 => option_or(Some(42), 0)\n"),
        (False, "enum Grow<T>\n    Item(Grow<[]T>)\nfn f(x: Grow<i64>)\n    println(0)\n"),
        (False, "struct Grow<T>\n    value: Grow<[]T>\nfn f(x: Grow<i64>)\n    println(0)\n"),
        (False, OPTION + "fn bad(x: Option<i64>) -> Result<i64, string>\n    let n = x?\n    Ok(n)\n" + RESULT),
        (True, "struct Pair<T>\n    values: []T\n    seed: T\nfn main() -> i64\n    let p = Pair { values: [], seed: 42 }\n    p.seed\n"),
        (True, "fn main()\n    let xs = []\n    println(nerv.arrays.len(xs))\n"),
        (False, "struct Box<T>\n    value: T\nstruct Box\n    value: i64\n"),
        (False, "struct Box\n    value: i64\nstruct Box<T>\n    value: T\n"),
        (False, "struct Pair<T>\n    values: []T\n    seed: T\nfn main()\n    let p = Pair { values: [], seed: true }\n"),
        (True, "enum Result<T, E>\n    Ok(E)\n    Err(T)\nfn f(x: Result<i64, string>) -> Result<i64, string>\n    let n: string = x?\n    Ok(n)\n"),
        (True, BOX + "fn main() -> i64 => Box { value: 42 }:get()\n"),
        (True, BOX + "fn main()\n    let a: Box<i64> = Box { value: 42 }\n    let b: Box<bool> = Box { value: true }\n    println(a:get())\n    println(b:get())\n"),
        (True, BOX + "fn main() -> i64 => Box { value: Box { value: 42 } }:get():get()\n"),
        (True, BOX + "fn unwrap<T>(box: Box<T>) -> T => box:get()\nfn main() -> i64 => unwrap(Box { value: 42 })\n"),
        (True, "struct Wrap<T>\n    value: T\n    fn copy(self) -> Wrap<T> => Wrap { value: self.value }\nfn main() -> i64 => Wrap { value: 42 }:copy().value\n"),
        (True, OPTION + "fn get(x: Option<i64>) -> i64\n    match x\n        Some(n) => n\n        None => 0\nfn main() -> i64 => get(Some(42))\n"),
        (True, OPTION + "fn absent() -> Option<i64> => None()\n"),
        (True, RESULT + "fn get() -> Result<i64, string> => Ok(41)\nfn increment() -> Result<i64, string>\n    let n = get()?\n    Ok(n + 1)\n"),
        (True, OPTION + "fn text(x: Option<i64>) -> Option<string>\n    let n = x?\n    Some(\"value\")\n"),
        (True, RESULT + "fn text(x: Result<i64, string>) -> Result<string, string>\n    let n = x?\n    Ok(\"value\")\n"),
        (True, "struct Node<T>\n    value: T\n    children: []Node<T>\nfn count(x: Node<i64>) -> i64 => nerv.arrays.len(x.children)\n"),
        (True, BOX + "fn main() -> i64\n    let mut b = Box { value: 1 }\n    b.value = 42\n    b.value\n"),
        (False, BOX + "fn main()\n    let b: Box<bool> = Box { value: 42 }\n"),
        (False, BOX + "fn main() => Box { missing: 42 }\n"),
        (False, BOX + "fn main() => Box { value: 1, value: 2 }\n"),
        (False, BOX + "fn main()\n    let b = Box { value: 1 }\n    b.value = 42\n"),
        (False, OPTION + "fn main()\n    let x = None()\n"),
        (False, OPTION + "fn bad(x: Option<i64>) -> i64 => x?\n"),
        (False, RESULT + "fn bad(x: Result<i64, string>) -> Result<i64, bool>\n    let n = x?\n    Ok(n)\n"),
        (False, "struct Node<T>\n    value: Node<T>\nfn f(x: Node<i64>)\n    println(0)\n"),
        (False, BOX + "fn f(x: Box<i64, bool>)\n    println(0)\n"),
        (False, "struct Empty<T>\n    n: i64\nfn main() => Empty { n: 0 }\n"),
    ]
    directory = ROOT / "target/nominal-cases"
    directory.mkdir(parents=True, exist_ok=True)
    for index, (accepted, source) in enumerate(cases):
        file = directory / f"case_{index}.nerv"
        file.write_text(source, encoding="utf-8", newline="")
        old = subprocess.run([str(stage0), str(file), "--check"], capture_output=True, text=True, encoding="utf-8", timeout=45)
        new = subprocess.run([str(compiler), "--ir", str(file)], capture_output=True, text=True, encoding="utf-8", timeout=45)
        expected = 0 if accepted else 1
        if old.returncode != expected or new.returncode != expected:
            raise RuntimeError(f"nominal mismatch: {file}\nreference {old.returncode}: {old.stderr}\nnew {new.returncode}: {new.stderr}")
        if accepted:
            validate_ir(new.stdout, file.read_bytes())
        elif new.stdout:
            raise RuntimeError(f"partial nominal IR: {file}")
    print(f"Matched stage 0 on {len(cases)} generic-type/propagation cases; checked IR validated")


if __name__ == "__main__":
    main()
