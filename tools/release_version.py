import argparse
import json
from pathlib import Path
import re
import tomllib


ROOT = Path(__file__).resolve().parent.parent
NUMBER = r"(?:0|[1-9][0-9]*)"
IDENTIFIER = rf"(?:{NUMBER}|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
SEMVER = re.compile(rf"({NUMBER})\.({NUMBER})\.({NUMBER})(?:-({IDENTIFIER}(?:\.{IDENTIFIER})*))?(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?", re.ASCII)
PLATFORMS = [
    {"os": "windows-2022", "target": "x86_64-pc-windows-gnullvm", "label": "windows-x64"},
    {"os": "macos-15", "target": "aarch64-apple-darwin", "label": "macos-arm64"},
    {"os": "ubuntu-24.04", "target": "x86_64-unknown-linux-gnu", "label": "linux-x64"},
]


def parse(version):
    match = SEMVER.fullmatch(version)
    if not match:
        raise ValueError(f"invalid Semantic Version: {version}")
    return match


def installer_version(version):
    match = parse(version)
    numbers = tuple(int(match[index]) for index in (1, 2, 3))
    if any(number > 65535 for number in numbers):
        raise ValueError("installer version components must fit in 16 bits")
    return ".".join(str(number) for number in (*numbers, 0))


def precedence(version):
    match = parse(version)
    identifiers = tuple((0, int(item)) if item.isdigit() else (1, item) for item in (match[4] or "").split(".") if item)
    return (int(match[1]), int(match[2]), int(match[3]), (0, identifiers) if match[4] else (1, ()))


def validate(root=ROOT, tag=None):
    with (root / "crates/nerv-cli/Cargo.toml").open("rb") as source:
        version = tomllib.load(source)["package"]["version"]
    match = parse(version)
    with (root / "Cargo.lock").open("rb") as source:
        packages = tomllib.load(source)["package"]
    versions = [package["version"] for package in packages if package["name"] == "nerv"]
    if versions != [version]:
        raise ValueError("compiler manifest and Cargo.lock versions disagree")
    if tag is not None and tag != f"v{version}":
        raise ValueError(f"release tag must be v{version}, got {tag}")
    return {"version": version, "prerelease": bool(match[4]), "installer_version": installer_version(version)}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--tag")
    parser.add_argument("--github-output", type=Path)
    parser.add_argument("--platform", choices=["all", *(item["label"] for item in PLATFORMS)], default="all")
    args = parser.parse_args()
    result = validate(tag=args.tag)
    selected = PLATFORMS if args.tag or args.platform == "all" else [item for item in PLATFORMS if item["label"] == args.platform]
    result["matrix"] = {"include": selected}
    if args.github_output:
        with args.github_output.open("a", encoding="utf-8") as output:
            for key, value in result.items():
                formatted = json.dumps(value) if isinstance(value, (dict, bool)) else str(value)
                output.write(f"{key}={formatted}\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
