import argparse
import os
from pathlib import Path
import struct
import subprocess


ROOT = Path(__file__).resolve().parent.parent
TARGETS = {
    "x86_64-pc-windows-gnu": ("coff", 0x8664),
    "aarch64-pc-windows-msvc": ("coff", 0xaa64),
    "x86_64-unknown-linux-gnu": ("elf", 62),
    "aarch64-unknown-linux-gnu": ("elf", 183),
    "x86_64-apple-darwin": ("macho", 0x01000007),
    "aarch64-apple-darwin": ("macho", 0x0100000c),
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--compiler", required=True, type=Path)
    args = parser.parse_args()
    compiler = args.compiler.resolve(strict=True)
    directory = ROOT / "target/target-cases"
    directory.mkdir(exist_ok=True)
    source = directory / "main.nerv"
    source.write_text('struct Pair\n    number: i64\n    text: string\nfn main() -> i64\n    let value = Pair { number: 42, text: "target" }\n    println(value.text)\n    value.number\n', encoding="utf-8")
    for target, (kind, expected) in TARGETS.items():
        output = directory / (target + ".o")
        env = dict(os.environ, NERV_TARGET=target)
        built = subprocess.run([str(compiler), "build", str(source), "--emit-object", "--release", "-o", str(output)], cwd=ROOT, env=env, capture_output=True, text=True, timeout=120)
        if built.returncode:
            raise RuntimeError(f"target {target}: {built.stderr}")
        data = output.read_bytes()
        if kind == "coff":
            actual = struct.unpack_from("<H", data)[0]
        elif kind == "elf":
            if data[:5] != b"\x7fELF\x02":
                raise RuntimeError("invalid ELF64 header")
            actual = struct.unpack_from("<H", data, 18)[0]
        else:
            if data[:4] != b"\xcf\xfa\xed\xfe":
                raise RuntimeError("invalid Mach-O 64 header")
            actual = struct.unpack_from("<I", data, 4)[0]
        if actual != expected:
            raise RuntimeError(f"target machine mismatch: {target} {actual}")
    print(f"Object targets passed: {len(TARGETS)} x64/ARM64 COFF, ELF, and Mach-O outputs; native execution is verified separately")


if __name__ == "__main__":
    main()
