import argparse
import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import time
import winreg


def user_path():
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, "Environment") as key:
        try:
            return winreg.QueryValueEx(key, "Path")[0]
        except FileNotFoundError:
            return ""


def run_installer(path, *arguments):
    subprocess.run([str(path), "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", *arguments], check=True)


def test(installer, payload):
    if os.environ.get("GITHUB_ACTIONS") != "true":
        raise ValueError("installer integration tests run only on disposable CI runners")
    before = user_path()
    with tempfile.TemporaryDirectory(prefix="nerv-install-test-") as temporary:
        destination = Path(temporary) / "Nerv install \u00e9"
        arguments = [f"/DIR={destination}", "/MERGETASKS=addpath"]
        run_installer(installer, *arguments)
        try:
            for original in payload.rglob("*"):
                if original.is_file():
                    installed = destination / original.relative_to(payload)
                    if hashlib.sha256(installed.read_bytes()).digest() != hashlib.sha256(original.read_bytes()).digest():
                        raise ValueError(f"installer changed payload: {original}")
            if str(destination).lower() not in user_path().lower().split(";"):
                raise ValueError("installer did not add its directory to PATH")
            project = destination / "my-project"
            project.mkdir()
            keep = project / "keep.txt"
            keep.write_text("user project", encoding="utf-8")
            run_installer(installer, *arguments)
            subprocess.run([str(destination / "nerv.exe"), "--version"], check=True)
            if keep.read_text(encoding="utf-8") != "user project":
                raise ValueError("reinstall changed user files")
        finally:
            run_installer(destination / "unins000.exe")
            for _ in range(50):
                if not (destination / "unins000.exe").exists():
                    break
                time.sleep(0.1)
        if user_path() != before:
            raise ValueError("uninstall did not preserve the original PATH")
        if keep.read_text(encoding="utf-8") != "user project":
            raise ValueError("uninstall removed user files")
        print("Installer payload, PATH, reinstall and uninstall verified")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--installer", type=Path, required=True)
    parser.add_argument("--payload", type=Path, required=True)
    args = parser.parse_args()
    test(args.installer.resolve(), args.payload.resolve())
