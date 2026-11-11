import argparse
import os
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parent.parent
CASES = [
    ('fn main() -> i64 => 42\n', 42, ''),
    ('fn main()\n    println(true)\n    println(false)\n', 0, 'true\nfalse\n'),
    ('import nerv.std\nfn worker(n: i64) -> i64 => n + 2\nfn main()\n    let task = result_or(spawn_task(40, worker), Task { id: 0 })\n    println(result_or(join_task(task), -1))\n    println(nerv.memory.scope(40, worker))\n', 0, '42\n42\n'),
    ('import nerv.std\nfn worker(n: i64) -> i64\n    while nerv.task.cancelled() == 0\n        assert(true)\n    n\nfn main()\n    let task = result_or(spawn_task(42, worker), Task { id: 0 })\n    assert(nerv.task.cancel(task.id) == 1)\n    println(result_or(join_task(task), -1))\n', 0, '42\n'),
    ('fn callback(handle: i64, kind: string, text: string, bytes: []u8, state: string) -> []string => [state]\nfn main()\n    let raw = nerv.net.websocket_run("not a url", [""] * 0, "state", 1, "", callback)\n    assert(nerv.string.contains(raw[1], "capture") == 0)\n', 0, ''),
    ('fn main()\n    println("hello")\n', 0, 'hello\n'),
    ('fn main()\n    println("caf\u00e9 \U0001f600")\n', 0, 'caf\u00e9 \U0001f600\n'),
    ('fn main()\n    println(-9223372036854775808 / -1)\n    println(-9223372036854775808 % -1)\n', 0, '-9223372036854775808\n0\n'),
    ('fn main()\n    println(i8(-128) / i8(-1))\n    println(u32(4294967295) / u32(3))\n', 0, '-128\n1431655765\n'),
    ('fn main()\n    let mut n = 0\n    for i in 0 .. 10\n        if i == 3\n            continue\n        if i == 7\n            break\n        n = n + i\n    println(n)\n', 0, '18\n'),
    ('fn main()\n    let mut n = 0\n    for i in 9223372036854775807 ..= 9223372036854775807\n        n = n + 1\n    println(n)\n', 0, '1\n'),
    ('fn side() -> bool\n    assert(false)\n    true\nfn main()\n    assert(not (false and side()))\n    assert(true or side())\n', 0, ''),
    ('fn get(n: bool) -> i64\n    if n\n        return 42\n    9\nfn main()\n    println(get(true))\n', 0, '42\n'),
    ('struct Pair\n    a: i64\n    b: i64\nfn main()\n    let mut value = Pair { b: 20, a: 22 }\n    value.a = 42\n    println(value.a + value.b)\n', 0, '62\n'),
    ('struct Box\n    value: i64\nfn main()\n    let mut values = [Box { value: 1 }] * 2\n    values[1] = Box { value: 42 }\n    println(values[1].value)\n', 0, '42\n'),
    ('import nerv.core\nfn a() -> Result<i64, string> => Err("bad")\nfn b() -> Result<bool, string>\n    let n = a()?\n    Ok(n == 1)\nfn main()\n    match b()\n        Ok(n) => println(n)\n        Err(e) => println(e)\n', 0, 'bad\n'),
    ('fn callback(n: i64) -> fn(i64) -> i64 => fn(x: i64) -> i64 => n + x\nfn main()\n    let f = callback(40)\n    println(f(2))\n', 0, '42\n'),
    ('import nerv.std\nfn add(a: i64, b: i64) -> i64 => a + b\nfn main()\n    println(nerv.arrays.reduce([20, 22], 2, 0, add))\n', 0, '42\n'),
    ('fn main()\n    let mut values = [1] * 2\n    let f = fn() -> i64 => values[0]\n    values[0] = 42\n    println(f())\n', 0, '42\n'),
    ('fn main()\n    println(i32(42.75))\n    println(u64(9223372036854775808.0))\n', 0, '42\n9223372036854775808\n'),
    ('fn main()\n    println(i8(128.0))\n', 1, ''),
    ('fn main()\n    let xs = [1]\n    println(xs[2])\n', 1, ''),
    ('fn main()\n    let zero = 0\n    println(1 / zero)\n', 1, ''),
    ('fn main()\n    assert(false)\n', 1, ''),
]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--stage0", required=True, type=Path)
    parser.add_argument("--compiler", required=True, type=Path)
    args = parser.parse_args()
    stage0 = args.stage0.resolve(strict=True)
    compiler = args.compiler.resolve(strict=True)
    fixtures = ROOT / "target/codegen-cases"
    fixtures.mkdir(exist_ok=True)
    suffix = ".exe" if os.name == "nt" else ""
    for index, (source, status, stdout) in enumerate(CASES):
        directory = fixtures / f"case_{index}"
        directory.mkdir(exist_ok=True)
        manifest = directory / "Nerv.toml"
        manifest.write_text(f'[package]\nname = "case_{index}"\nversion = "0.1.0"\nentry = "main.nerv"\n', encoding="utf-8")
        file = directory / "main.nerv"
        file.write_text(source, encoding="utf-8")
        for mode in ["--debug", "--release"]:
            for label, driver in [("reference", stage0), ("self", compiler)]:
                output = fixtures / (label + suffix)
                built = subprocess.run([str(driver), "build", "--manifest-path", str(manifest), mode, "-o", str(output)], cwd=ROOT, capture_output=True, text=True, encoding="utf-8", timeout=120)
                if built.returncode:
                    raise RuntimeError(f"build {index} {mode} {label}: {built.stderr}")
                result = subprocess.run([str(output)], cwd=ROOT, capture_output=True, text=True, encoding="utf-8", timeout=30)
                if result.returncode != status or result.stdout != stdout:
                    raise RuntimeError(f"execution {index} {mode} {label}: {result.returncode} {result.stdout!r} {result.stderr}")
    failing = fixtures / "failing_test.nerv"
    failing.write_text('test "fails"\n    assert(false)\n', encoding="utf-8")
    tested = subprocess.run([str(compiler), "test", str(failing), "-o", str(fixtures / ("tests" + suffix))], cwd=ROOT, capture_output=True, text=True, encoding="utf-8", timeout=120)
    if tested.returncode != 1 or "assertion failed" not in tested.stderr:
        raise RuntimeError("test runner failed to propagate a failing assertion")
    print(f"Code generation passed: {len(CASES)} executable cases in debug and release, compared with stage 0; failing tests propagate")


if __name__ == "__main__":
    main()
