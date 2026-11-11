import os
import argparse
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--stage0", required=True, type=Path)
    parser.add_argument("--compiler", required=True, type=Path)
    args = parser.parse_args()
    compilers = [args.stage0.resolve(strict=True), args.compiler.resolve(strict=True)]
    sources = sorted((ROOT / "src").glob("*.nerv")) + sorted((ROOT / "tests").glob("*.nerv")) + sorted((ROOT / "stdlib").glob("*.nerv")) + sorted((Path(os.environ.get("NERV_REFERENCE", str(ROOT.parent / "Nerv-v2"))) / "example").rglob("*.nerv"))
    directory = ROOT / "target/format-cases"
    directory.mkdir(exist_ok=True)
    for index, source in enumerate(sources):
        results = []
        for label, compiler in enumerate(compilers):
            file = directory / f"case_{index}_{label}.nerv"
            file.write_bytes(source.read_bytes())
            result = subprocess.run([str(compiler), "fmt", str(file)], cwd=ROOT, capture_output=True, text=True, encoding="utf-8", timeout=30)
            if result.returncode:
                raise RuntimeError(f"formatting {source} with {label}: {result.stderr}")
            results.append(file.read_bytes())
            checked = subprocess.run([str(compiler), "fmt", str(file), "--check"], cwd=ROOT, capture_output=True, text=True, encoding="utf-8", timeout=30)
            if checked.returncode:
                raise RuntimeError(f"formatter is not idempotent: {source} {checked.stderr}")
        if results[0] != results[1]:
            raise RuntimeError(f"formatter mismatch: {source}; compare {directory}/case_{index}_*.nerv")
    print(f"Formatting matched stage 0 on {len(sources)} files; idempotence checks passed")


if __name__ == "__main__":
    main()
