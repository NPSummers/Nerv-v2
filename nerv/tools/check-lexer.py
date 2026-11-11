import os
import argparse
from pathlib import Path
import random
import re
import subprocess


ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--stage0", required=True, type=Path)
    parser.add_argument("--lexer", required=True, type=Path)
    parser.add_argument("--reference", type=Path, default=Path(os.environ.get("NERV_REFERENCE", str(ROOT.parent / "Nerv-v2"))))
    args = parser.parse_args()
    stage0 = args.stage0.resolve(strict=True)
    lexer = args.lexer.resolve(strict=True)
    sources = sorted((args.reference / "example").rglob("*.nerv"))
    sources += sorted((args.reference / "stdlib").glob("*.nerv"))
    sources += sorted((ROOT / "src").glob("*.nerv"))
    sources += sorted((ROOT / "tests").glob("*.nerv"))
    cases = ["", "-- comment", "\n\t-- blank\n", '"Ã©ä¸–ç•ŒðŸ˜€" name\n', "'Ã©' 'x' '\\n'\n",
             "1 1_000 1.2 1e2 1.2e-3 1..2 1..=2 1.e2 1e+ 1._ 1e_\n",
             "fn main()\r\n\t[1,\r\n 2]\r\n", "a\rb\r", "a\n    b\n        c\n    d\ne",
             "let mut fn struct enum trait impl import export module use if else for in while match test bench unsafe return break continue and or not true false Some None implements\n",
             "+ - * / % == != < > <= >= = => -> |> ( ) [ ] { } , . .. ..= : :: ; & | ?\n",
             "@", "!", "Ã©", "\"unfinished", "'ab'", "fn f()\n    x\n  y", '"bad\nstring"', "\x00"]
    randomizer = random.Random(42)
    atoms = ["x", "1", "2.0", "Some(3)", '"hello"', "true", "1..=4"]
    for _ in range(40):
        cases.append("fn main()\n    let x = [\n        " + ", ".join(randomizer.choices(atoms, k=5)) + "\n    ]\n    x\n")

    def compare(file):
        old = subprocess.run([str(stage0), str(file), "--lex"], capture_output=True, text=True, encoding="utf-8", timeout=30)
        new = subprocess.run([str(lexer), "--lex", str(file)], capture_output=True, text=True, encoding="utf-8", timeout=30)
        if old.returncode == 0:
            if new.returncode != 0 or old.stdout != new.stdout:
                raise RuntimeError(f"token mismatch: {file}\nreference:\n{old.stdout}\nnew:\n{new.stdout}\n{new.stderr}")
            spans = subprocess.run([str(lexer), "--tokens", str(file)], capture_output=True, text=True, encoding="utf-8", timeout=30)
            if spans.returncode != 0:
                raise RuntimeError(spans.stderr)
            raw = file.read_bytes()
            reconstructed = []
            for entry in spans.stdout.splitlines():
                kind, start, end, line, column = entry.split("\t")
                start, end, line, column = map(int, (start, end, line, column))
                if not 0 <= start <= end <= len(raw):
                    raise RuntimeError(f"invalid byte span: {file}: {entry}")
                prefix = raw[:start].decode("utf-8")
                if (line, column) != (prefix.count("\n") + 1, len(prefix.rsplit("\n", 1)[-1]) + 1):
                    raise RuntimeError(f"invalid source location: {file}: {entry}")
                reconstructed.append(f"{line}:{column} {kind} {raw[start:end].decode('utf-8')}\n")
            if "".join(reconstructed).replace("\r\n", "\n").replace("\r", "\n") != old.stdout:
                raise RuntimeError(f"byte spans disagree with stage 0 token text: {file}")
        else:
            location = re.search(r":(\d+):(\d+): error:", old.stderr)
            if new.returncode != 1 or location is None or not new.stderr.startswith(f"{location[1]}:{location[2]}:"):
                raise RuntimeError(f"diagnostic mismatch: {file}\nreference:\n{old.stderr}\nnew:\n{new.stderr}")

    for file in sources:
        compare(file.resolve())
    fixtures = ROOT / "target/lexer-cases"
    fixtures.mkdir(parents=True, exist_ok=True)
    for index, text in enumerate(cases):
        file = fixtures / f"case_{index}.nerv"
        file.write_text(text, encoding="utf-8", newline="")
        compare(file)
    print(f"Matched stage 0 on {len(sources)} source files and {len(cases)} focused/generated cases")


if __name__ == "__main__":
    main()
