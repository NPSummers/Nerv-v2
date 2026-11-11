import argparse
import json
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parent.parent


def validate_ir(text, source):
    rows = [json.loads(line) for line in text.splitlines()]
    types = {row[1]: row for row in rows if row[0] == "type"}
    symbols = {row[1]: row for row in rows if row[0] == "symbol"}
    nodes = {row[1]: row for row in rows if row[0] == "node"}
    sources = {row[1]: Path(row[2]).read_bytes() for row in rows if row[0] == "source"}
    if not rows or rows[-1][0] != "program":
        raise ValueError("missing checked program")
    for row in types.values():
        if any(part not in types for part in row[5]):
            raise ValueError(f"invalid type argument: {row}")
        if row[2] == "Nominal" and row[4] not in symbols:
            raise ValueError(f"invalid nominal identity: {row}")
    for row in symbols.values():
        if len(row) > 8 and row[8] not in sources:
            raise ValueError(f"invalid symbol source: {row}")
        if row[4] not in types or row[4] == 6:
            raise ValueError(f"unresolved symbol type: {row}")
        if row[5] >= 0 and row[5] not in symbols:
            raise ValueError(f"invalid symbol owner: {row}")
    for row in nodes.values():
        _, index, op, ty, symbol, slot, start, end, text, children = row[:10]
        if len(row) > 10 and row[10] not in sources:
            raise ValueError(f"invalid node source: {row}")
        unit = sources.get(row[10], source) if len(row) > 10 else source
        if ty not in types or ty == 6 or not 0 <= start <= end <= len(unit):
            raise ValueError(f"invalid checked node: {row}")
        unit[:start].decode("utf-8")
        unit[:end].decode("utf-8")
        if symbol >= 0 and symbol not in symbols:
            raise ValueError(f"invalid resolved symbol: {row}")
        if any(child not in nodes or child >= index for child in children):
            raise ValueError(f"invalid checked children: {row}")
        if op in {"Local", "FunctionRef", "Call", "Let", "Function", "Parameter", "Enum", "Struct", "Builtin", "Cast", "Lambda"} and symbol < 0:
            raise ValueError(f"unresolved operation: {row}")
    for row in rows:
        if row[0] == "capture":
            if any(symbol not in symbols for symbol in row[1:4]):
                raise ValueError(f"invalid capture mapping: {row}")
    main, functions = rows[-1][1:]
    if main >= 0 and (main not in symbols or symbols[main][3] != "function"):
        raise ValueError("invalid main symbol")
    if any(root not in nodes or nodes[root][2] != "Function" for root in functions):
        raise ValueError("invalid function roots")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--stage0", required=True, type=Path)
    parser.add_argument("--compiler", required=True, type=Path)
    args = parser.parse_args()
    stage0 = args.stage0.resolve(strict=True)
    compiler = args.compiler.resolve(strict=True)
    accepted = [
        "fn main() => 42\n",
        "fn identity<T>(value: T) -> T => value\nfn main() => identity(42)\n",
        "struct main\n    value: i64\n",
        "fn main() -> i64\n    let n = 1\n    let outer = fn() -> fn() -> i64 => fn() -> i64 => n\n    let inner = outer()\n    inner()\n",
        "fn main() -> i64\n    return 1\n    true\n",
        "fn main() -> i64 => 42\n",
        "fn main()\n    println(\"Hello\")\n",
        "fn f(n: i64) -> i64 => n + 1\nfn main() -> i64 => f(41)\n",
        "fn main() -> i64\n    let value = 40\n    let value = value + 2\n    value\n",
        "fn main() -> i64\n    let mut value = 40\n    value = value + 2\n    value\n",
        "fn main() -> i64\n    let values: []i64 = []\n    nerv.arrays.len(values)\n",
        "fn main() -> i64 => [1, 2][0]\n",
        "fn main() -> i64 => nerv.arrays.len([1] * 3)\n",
        "fn main() -> i64\n    let mut a = [1, 2]\n    a[0] = 42\n    a[0]\n",
        "fn main() -> i64\n    if true\n        return 1\n    else\n        return 2\n",
        "fn main() -> i64\n    let mut n = 0\n    while n < 4\n        n = n + 1\n    n\n",
        "fn main() -> i64\n    let mut n = 0\n    for i in 0 ..= 4\n        if i == 2\n            continue\n        n = n + i\n    n\n",
        "struct S\n    value: i64\nfn main() -> i64 => S { value: 42 }.value\n",
        "struct S\n    value: i64\nfn main() -> i64\n    let mut s = S { value: 1 }\n    s.value = 42\n    s.value\n",
        "struct S\n    value: i64\n    fn get(self) -> i64 => self.value\nfn main() -> i64 => S { value: 42 }:get()\n",
        "enum E\n    Number(i64)\n    Empty\nfn f(x: E) -> i64\n    match x\n        Number(n) => n\n        Empty => 0\nfn main() -> i64 => f(E.Number(42))\n",
        "fn make(base: i64) -> fn(i64) -> i64\n    fn(n: i64) -> i64 => base + n\nfn main() -> i64\n    let add = make(40)\n    add(2)\n",
        "fn main() -> i64\n    let mut value = 10\n    let snapshot = fn() -> i64 => value\n    value = 20\n    snapshot()\n",
        "fn f(x: i32) -> i32 => x + i32(2)\nfn main() -> i64 => i64(f(i32(40)))\n",
        "fn main()\n    assert(\"ÃƒÂ©\" == nerv.string.concat(\"\", \"ÃƒÂ©\"))\n",
        "test \"works\"\n    assert(1 + 1 == 2)\n",
    ]
    rejected = [
        "enum E\n    Item(E)\n",
        "enum E\n    Item(i64)\nfn main()\n    assert(Item(1) == Item(1))\n",
        "enum E\n    A\n    B\nfn f(x: E) -> i64\n    match x\n        _ => 42\nfn main() -> i64 => f(A())\n",
        "fn main() -> i64 => missing\n",
        "fn main() -> i64 => 1 + true\n",
        "fn main() -> i64 => 1 + 2.0\n",
        "fn main() -> i64 => true\n",
        "fn main(n: i64) -> i64 => n\n",
        "fn f(x: void) => 0\n",
        "fn f(x: i64, x: i64) -> i64 => x\n",
        "fn f() => 0\nfn f() => 0\n",
        "fn main()\n    let x: f64 = 1\n",
        "fn main()\n    let x = 1\n    x = 2\n",
        "fn main()\n    let mut x = 1\n    x = false\n",
        "fn main() -> i64\n    if true\n        let x = 1\n    x\n",
        "fn main()\n    break\n",
        "fn main()\n    continue\n",
        "fn main()\n    while 1\n        break\n",
        "fn main()\n    for n in true .. false\n        println(n)\n",
        "fn main()\n    return 1\n",
        "fn main() -> i64\n    return\n",
        "fn main() -> i64\n    if true\n        1\n    else\n        false\n",
        "fn f(x: i64) -> i64 => x\nfn main() -> i64 => f(true)\n",
        "fn f(x: i64) -> i64 => x\nfn main() -> i64 => f()\n",
        "fn main() -> i64 => 1()\n",
        "fn main()\n    let xs = [1, true]\n",
        "fn main() -> i64 => [1][true]\n",
        "fn main() -> i64 => 1[0]\n",
        "fn main()\n    let xs = [1]\n    xs[0] = 2\n",
        "struct S\n    value: void\n",
        "struct S\n    value: S\n",
        "struct S\n    value: i64\n    value: i64\n",
        "struct S\n    value: i64\nfn main() => S { value: true }\n",
        "struct S\n    value: i64\nfn main() => S { other: 42 }\n",
        "struct S\n    value: i64\nfn main()\n    let s = S { value: 1 }\n    s.value = 2\n",
        "fn main() => Missing { value: 1 }\n",
        "enum E\n    A\n    B\nfn f(x: E) -> i64\n    match x\n        A => 1\n",
        "enum E\n    A\nfn f(x: E) -> i64\n    match x\n        A(n) => n\n",
        "fn main()\n    assert(1)\n",
        "fn main()\n    println([1])\n",
        "fn main() -> i64 => i64(true)\n",
    ]
    fixtures = ROOT / "target/sema-cases"
    fixtures.mkdir(parents=True, exist_ok=True)
    operations = 0
    for index, text in enumerate(accepted + rejected):
        file = fixtures / f"case_{index}.nerv"
        file.write_text(text, encoding="utf-8", newline="")
        old = subprocess.run([str(stage0), str(file), "--check"], capture_output=True, text=True, encoding="utf-8", timeout=30)
        new = subprocess.run([str(compiler), "--ir", str(file)], capture_output=True, text=True, encoding="utf-8", timeout=30)
        expected = 0 if index < len(accepted) else 1
        if old.returncode != expected or new.returncode != expected:
            raise RuntimeError(f"semantic mismatch: {file}\nexpected: {expected}\nreference: {old.returncode}: {old.stderr}\nnew: {new.returncode}: {new.stderr}")
        if expected == 0:
            validate_ir(new.stdout, file.read_bytes())
            operations += sum(json.loads(line)[0] == "node" for line in new.stdout.splitlines())
        elif new.stdout:
            raise RuntimeError(f"partial checked IR escaped from failed analysis: {file}")
    unsupported = ["trait Read\n    fn read(self) -> i64\nfn use_trait(value: Read)\n    println(0)\n"]
    for index, text in enumerate(unsupported):
        file = fixtures / f"unsupported_{index}.nerv"
        file.write_text(text, encoding="utf-8")
        result = subprocess.run([str(compiler), "--ir", str(file)], capture_output=True, text=True, timeout=30)
        if result.returncode != 1 or result.stdout or "not implemented" not in result.stderr:
            raise RuntimeError(f"unsupported feature did not fail explicitly: {file}: {result}")
    print(f"Matched stage 0 on {len(accepted)} accepted and {len(rejected)} rejected semantic cases; validated {operations} checked operations; unsupported-feature checks passed")


if __name__ == "__main__":
    main()
