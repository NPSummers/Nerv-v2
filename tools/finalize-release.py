import argparse
import hashlib
import json
from pathlib import Path


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def finalize(directory, payload):
    info = json.loads((payload / "release-info.json").read_text(encoding="utf-8"))
    files = sorted(path for path in directory.iterdir() if path.suffix in {".exe", ".pkg", ".zip"} or path.name.endswith(".tar.gz"))
    expected = 1 if info["platform"].startswith("linux") else 2
    if len(files) != expected:
        raise ValueError(f"expected {expected} final artifacts, found {len(files)}")
    for path in files:
        if not path.name.startswith(f"nerv-{info['version']}-{info['platform']}-"):
            raise ValueError(f"artifact identity mismatch: {path}")
    info["artifacts"] = [{"name": path.name, "size": path.stat().st_size, "sha256": digest(path)} for path in files]
    result = directory / f"manifest-{info['platform']}.json"
    result.write_text(json.dumps(info, indent=2) + "\n", encoding="utf-8")
    for path in directory.glob("*.sha256"):
        path.unlink()
    print(result)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--payload", type=Path, required=True)
    args = parser.parse_args()
    finalize(args.directory, args.payload)
