import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

from release_version import parse, precedence


PLATFORMS = {"windows-x64", "macos-arm64", "linux-x64"}


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def inventory(directory, version, commit):
    manifests = [json.loads(path.read_text(encoding="utf-8")) for path in sorted(directory.glob("manifest-*.json"))]
    if len(manifests) != 3 or {item["platform"] for item in manifests} != PLATFORMS:
        raise ValueError("all three required platform manifests must be present")
    artifacts = []
    for item in manifests:
        if item["version"] != version or item["source_commit"] != commit:
            raise ValueError("release manifests disagree on version or source commit")
        if item.get("source_dirty", True):
            raise ValueError("release payload must come from an unmodified tagged checkout")
        if item["platform"] == "macos-arm64" and item["signing"] not in {"unsigned", "Developer ID, notarized"}:
            raise ValueError("unrecognized macOS signing status")
        expected = 1 if item["platform"] == "linux-x64" else 2
        if len(item["artifacts"]) != expected:
            raise ValueError("incomplete platform artifact set")
        suffixes = {"windows-x64": ["portable.zip", "setup-unsigned.exe"], "macos-arm64": ["portable.zip", "installer.pkg"], "linux-x64": ["portable.tar.gz"]}[item["platform"]]
        if item["platform"] == "macos-arm64" and item["signing"] == "unsigned":
            suffixes = ["portable.zip", "installer-unsigned.pkg"]
        if {artifact["name"] for artifact in item["artifacts"]} != {f"nerv-{version}-{item['platform']}-{suffix}" for suffix in suffixes}:
            raise ValueError("platform is missing an expected installer/portable download")
        for artifact in item["artifacts"]:
            name = artifact["name"]
            if Path(name).name != name or not name.startswith(f"nerv-{version}-{item['platform']}-"):
                raise ValueError("invalid release asset name")
            path = directory / name
            if digest(path) != artifact["sha256"] or path.stat().st_size != artifact["size"]:
                raise ValueError(f"release asset hash/size mismatch: {name}")
            artifacts.append(artifact)
    if len({item["name"] for item in artifacts}) != 5:
        raise ValueError("release must contain five distinct downloads")
    result = {"version": version, "source_commit": commit, "platforms": manifests}
    (directory / "release-manifest.json").write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    assets = [directory / item["name"] for item in artifacts] + [directory / "release-manifest.json"]
    sums = directory / "SHA256SUMS"
    sums.write_text("".join(f"{digest(path)}  {path.name}\n" for path in sorted(assets)), encoding="ascii")
    return assets + [sums]


def verify_tag(repo, tag, commit):
    reference = json.loads(subprocess.check_output(["gh", "api", f"repos/{repo}/git/ref/tags/{tag}"], text=True))["object"]
    for _ in range(8):
        if reference["type"] == "commit":
            if reference["sha"] != commit:
                raise ValueError("remote release tag moved away from the tested commit")
            return
        if reference["type"] != "tag":
            break
        reference = json.loads(subprocess.check_output(["gh", "api", f"repos/{repo}/git/tags/{reference['sha']}"], text=True))["object"]
    raise ValueError("release tag does not resolve to a commit")


def publish(directory, tag, commit):
    if os.environ.get("GITHUB_EVENT_NAME") != "push" or os.environ.get("GITHUB_REF") != f"refs/tags/{tag}":
        raise ValueError("publication requires a tag push event")
    if not tag.startswith("v"):
        raise ValueError("release tag requires v prefix")
    match = parse(tag[1:])
    assets = inventory(directory, tag[1:], commit)
    repo = os.environ["GH_REPO"]
    verify_tag(repo, tag, commit)
    result = subprocess.run(["gh", "api", f"repos/{repo}/releases/tags/{tag}"], capture_output=True, text=True)
    release = None
    if result.returncode:
        if "HTTP 404" not in result.stderr:
            raise RuntimeError(result.stderr)
    else:
        release = json.loads(result.stdout)
    with tempfile.TemporaryDirectory(prefix="nerv-release-") as temporary:
        root = Path(temporary)
        notes = root / "notes.md"
        notes.write_text(f"Nerv {tag[1:]}\n\n| Platform | Downloads | Minimum system |\n| --- | --- | --- |\n| Windows x64 | Setup EXE (unsigned), portable ZIP | Windows 10 / UCRT |\n| macOS arm64 | Installer PKG, portable ZIP | macOS 15 |\n| Linux x64 | Portable tar.gz | glibc 2.39 |\n\nVerify downloads using SHA256SUMS. Extract portable downloads or run the installer, then use `nerv new hello` and `nerv run`. No separate compiler toolchain is needed. Windows installers are unsigned; inspect the signing status in release-manifest.json.\n\nSee the documented language and release compatibility policy. Source commit: {commit}.\n", encoding="utf-8")
        release_notes = Path(__file__).resolve().parent.parent / "docs" / f"releases-{tag[1:]}.md"
        if release_notes.is_file():
            body = release_notes.read_text(encoding="utf-8").replace("](installation.md)", f"](https://github.com/{repo}/blob/{commit}/docs/installation.md)")
            notes.write_text(body + f"\nSource commit: {commit}.\n", encoding="utf-8")
        manifests = [json.loads(path.read_text(encoding="utf-8")) for path in directory.glob("manifest-*.json")]
        unsigned = [item["platform"] for item in manifests if item["signing"] == "unsigned" and item["platform"] in {"windows-x64", "macos-arm64"}]
        if unsigned:
            with notes.open("a", encoding="utf-8") as output:
                output.write("\nSigning status: " + ", ".join(sorted(unsigned)) + " builds are unsigned. The macOS package is not Apple-notarized. Installer filenames include `unsigned`; full signing details are in release-manifest.json.\n")
        if release is None:
            subprocess.run(["gh", "release", "create", tag, "--verify-tag", "--draft", "--title", f"Nerv {tag[1:]}", "--notes-file", str(notes)], check=True)
            release = {"draft": True, "assets": []}
        existing = {item["name"] for item in release["assets"]}
        expected = {path.name for path in assets}
        if existing - expected:
            raise ValueError("existing release contains unexpected assets")
        for asset in assets:
            if asset.name in existing:
                subprocess.run(["gh", "release", "download", tag, "--pattern", asset.name, "--dir", str(root)], check=True)
                if digest(root / asset.name) != digest(asset):
                    raise ValueError(f"refusing to replace existing release asset: {asset.name}")
            elif release["draft"]:
                subprocess.run(["gh", "release", "upload", tag, str(asset)], check=True)
            else:
                raise ValueError(f"published release is missing {asset.name}; publish a new version")
        if release["draft"]:
            latest = not match[4]
            if latest:
                current = subprocess.run(["gh", "api", f"repos/{repo}/releases/latest"], capture_output=True, text=True)
                if current.returncode == 0:
                    previous = json.loads(current.stdout)["tag_name"]
                    latest = previous.startswith("v") and precedence(tag[1:]) > precedence(previous[1:])
                elif "HTTP 404" not in current.stderr:
                    raise RuntimeError(current.stderr)
            subprocess.run(["gh", "release", "edit", tag, "--draft=false", f"--prerelease={'true' if match[4] else 'false'}", f"--latest={str(latest).lower()}", "--notes-file", str(notes)], check=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--commit", required=True)
    args = parser.parse_args()
    publish(args.directory, args.tag, args.commit)
