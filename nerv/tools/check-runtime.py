import argparse
import json
from pathlib import Path
import re
import runpy
import subprocess


ROOT = Path(__file__).resolve().parent.parent
validate_ir = runpy.run_path(str(Path(__file__).with_name("check-sema.py")))["validate_ir"]
HELPERS = "fn worker(n: i64) -> i64 => n\nfn combine(a: i64, b: i64) -> i64 => a + b\nfn web(kind: i64, text: string, data: string, bytes: []u8, state: string) -> []string => [state]\n"
ARGUMENT = {"i": "0", "f": "0.0", "s": '""', "b": "false", "a": "[0]", "S": '[""]', "B": "[u8(0)]", "C": "worker", "D": "combine", "W": "web"}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--stage0", required=True, type=Path)
    parser.add_argument("--compiler", required=True, type=Path)
    args = parser.parse_args()
    stage0 = args.stage0.resolve(strict=True)
    compiler = args.compiler.resolve(strict=True)
    catalog = (ROOT / "src/runtime.nerv").read_text(encoding="utf-8")
    signatures = re.findall(r'if name == "([^"]+)"\n\s+return "([^"]*>[^"]+)"', catalog)
    if not signatures:
        raise ValueError("runtime catalog not found")
    reference = ROOT / "bootstrap/crates/nerv-runtime/src"
    exports = set()
    for file in reference.glob("*.rs"):
        source = file.read_text(encoding="utf-8")
        exports.update(re.findall(r'name: "([^"]+)"', source))
        exports.update(re.findall(r'extern "C" fn (\w+)', source))
    directory = ROOT / "target/runtime-cases"
    directory.mkdir(parents=True, exist_ok=True)
    cases = []
    for name, signature in signatures:
        parameters = signature.split(">")[0]
        args_text = ", ".join(ARGUMENT[part] for part in parameters.split(",")) if parameters else ""
        cases.append((True, f"{name}({args_text})", None))
    for expression, abi in [("println(i8(1))", "nerv_println_i64"), ("println(u8(1))", "nerv_println_u64"), ("println(f32(1.0))", "nerv_println_f64"), ('println("text")', "nerv_println_string"), ("println(true)", "nerv_println_bool"), ("nerv.math.abs(-1)", "nerv_abs_i64"), ("nerv.math.abs(-1.0)", "nerv_abs_f64")]:
        cases.append((True, expression, abi))
    for expression in ['nerv.fs.read_text(1)', 'nerv.io.now_ms(1)', 'nerv.json.bool("", 1)', 'nerv.net.websocket_send_binary(0, [1])', 'nerv.task.spawn(0, combine)', 'nerv.arrays.map([1], 1, combine)', 'nerv.math.abs(i8(1))', 'nerv.unknown.operation()']:
        cases.append((False, expression, None))
    for index, (accepted, expression, expected_abi) in enumerate(cases):
        file = directory / f"case_{index}.nerv"
        text = HELPERS + "fn main()\n    " + expression + "\n"
        file.write_text(text, encoding="utf-8", newline="")
        old = subprocess.run([str(stage0), str(file), "--check"], capture_output=True, text=True, encoding="utf-8", timeout=30)
        new = subprocess.run([str(compiler), "--ir", str(file)], capture_output=True, text=True, encoding="utf-8", timeout=30)
        expected = 0 if accepted else 1
        if old.returncode != expected or new.returncode != expected:
            raise RuntimeError(f"runtime mismatch: {expression}\nreference {old.returncode}: {old.stderr}\nnew {new.returncode}: {new.stderr}")
        if accepted:
            validate_ir(new.stdout, file.read_bytes())
            symbols = [json.loads(line) for line in new.stdout.splitlines() if json.loads(line)[0] == "symbol"]
            native = [symbol[2] for symbol in symbols if symbol[3] == "builtin"]
            if any(name not in exports for name in native):
                raise RuntimeError(f"unexported runtime ABI: {native}")
            if expected_abi and expected_abi not in native:
                raise RuntimeError(f"wrong ABI for {expression}: {native}")
        elif new.stdout:
            raise RuntimeError("partial IR for rejected runtime call")
    print(f"Matched stage 0 on {len(cases)} runtime-call cases; {len(signatures)} catalog signatures and native ABI names checked. No I/O or network calls executed.")


if __name__ == "__main__":
    main()
