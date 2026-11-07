import argparse
from pathlib import Path
import shutil
import subprocess
import tempfile

from release_version import installer_version, parse


ROOT = Path(__file__).resolve().parent.parent


def windows(payload, output, version, iscc):
    subprocess.run([str(iscc), "/Qp", f"/DPayload={payload}", f"/DReleaseVersion={version}", f"/DNumericVersion={installer_version(version)}", f"/DOutputDirectory={output}", str(ROOT / "tools/installers/windows.iss")], check=True)
    return output / f"nerv-{version}-windows-x64-setup-unsigned.exe"


def macos(payload, output, version):
    with tempfile.TemporaryDirectory(prefix="nerv-installer-") as directory:
        root = Path(directory)
        content = root / "payload"
        location = f"/Library/Nerv/{version}"
        shutil.copytree(payload, content / location.lstrip("/"), symlinks=True)
        scripts = root / "scripts"
        scripts.mkdir()
        postinstall = scripts / "postinstall"
        postinstall.write_text('#!/bin/sh\nset -eu\nroot="${3:-/}"\nlink="$root/usr/local/bin/nerv"\nmkdir -p "$root/usr/local/bin"\nif [ -e "$link" ] || [ -L "$link" ]; then\n  if [ ! -L "$link" ]; then echo "Refusing to replace an existing non-symlink nerv command" >&2; exit 1; fi\n  case "$(readlink "$link")" in /Library/Nerv/*/nerv) ;; *) echo "Existing nerv symlink belongs to another installation" >&2; exit 1 ;; esac\nfi\nln -sfn ' + f"'/Library/Nerv/{version}/nerv' \"$link\"\n", encoding="utf-8")
        postinstall.chmod(0o755)
        identifier = "org.nerv.compiler.v" + version.encode().hex()
        component = root / "component.pkg"
        subprocess.run(["pkgbuild", "--root", str(content), "--identifier", identifier, "--version", installer_version(version), "--scripts", str(scripts), "--ownership", "recommended", str(component)], check=True)
        result = output / f"nerv-{version}-macos-arm64-installer-unsigned.pkg"
        subprocess.run(["productbuild", "--identifier", identifier, "--version", installer_version(version), "--package", str(component), str(result)], check=True)
        return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--payload", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--platform", choices=["windows", "macos"], required=True)
    parser.add_argument("--iscc", type=Path)
    args = parser.parse_args()
    parse(args.version)
    payload = args.payload.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    if (payload / "VERSION").read_text(encoding="utf-8").splitlines()[0] != args.version:
        raise ValueError("installer version does not match payload")
    if args.platform == "windows":
        iscc = args.iscc or shutil.which("ISCC.exe")
        if not iscc:
            raise ValueError("pass --iscc pointing to the pinned Inno Setup compiler")
        result = windows(payload, output, args.version, iscc)
    else:
        result = macos(payload, output, args.version)
    print(result)


if __name__ == "__main__":
    main()
