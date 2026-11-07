import argparse
import base64
import importlib.util
import json
import os
from pathlib import Path
import secrets
import subprocess
import tempfile


def run(*arguments):
    subprocess.run(arguments, check=True)


def notarize(path, authentication):
    result = subprocess.check_output(["xcrun", "notarytool", "submit", str(path), *authentication, "--wait", "--timeout", "30m", "--output-format", "json"], text=True)
    response = json.loads(result)
    if response.get("status") != "Accepted":
        raise RuntimeError(f"notarization failed: {response.get('id')}: {response.get('status')}")


def sign(payload, directory, version):
    required = ["MACOS_CERTIFICATE_P12", "MACOS_CERTIFICATE_PASSWORD", "MACOS_APPLICATION_IDENTITY", "MACOS_INSTALLER_IDENTITY", "APPLE_ID", "APPLE_APP_PASSWORD", "APPLE_TEAM_ID"]
    missing = [key for key in required if not os.environ.get(key)]
    if missing:
        raise ValueError("public macOS releases need protected signing secrets: " + ", ".join(missing))
    with tempfile.TemporaryDirectory(prefix="nerv-signing-") as temporary:
        root = Path(temporary)
        certificate = root / "identities.p12"
        certificate.write_bytes(base64.b64decode(os.environ["MACOS_CERTIFICATE_P12"], validate=True))
        certificate.chmod(0o600)
        keychain = root / "release.keychain-db"
        password = secrets.token_urlsafe(32)
        run("security", "create-keychain", "-p", password, str(keychain))
        try:
            run("security", "set-keychain-settings", "-lut", "21600", str(keychain))
            run("security", "unlock-keychain", "-p", password, str(keychain))
            run("security", "import", str(certificate), "-k", str(keychain), "-P", os.environ["MACOS_CERTIFICATE_PASSWORD"], "-T", "/usr/bin/codesign", "-T", "/usr/bin/productsign")
            run("security", "set-key-partition-list", "-S", "apple-tool:,apple:,codesign:", "-s", "-k", password, str(keychain))
            entitlements = root / "entitlements.plist"
            entitlements.write_text('<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd"><plist version="1.0"><dict><key>com.apple.security.cs.allow-unsigned-executable-memory</key><true/></dict></plist>', encoding="utf-8")
            for path in sorted(payload.rglob("*")):
                if not path.is_file():
                    continue
                with path.open("rb") as source:
                    magic = source.read(4)
                if magic in {b"\xcf\xfa\xed\xfe", b"\xce\xfa\xed\xfe", b"\xca\xfe\xba\xbe"}:
                    options = ["--entitlements", str(entitlements)] if path == payload / "nerv" else []
                    run("codesign", "--force", "--options", "runtime", "--timestamp", "--keychain", str(keychain), "--sign", os.environ["MACOS_APPLICATION_IDENTITY"], *options, str(path))
                    run("codesign", "--verify", "--strict", str(path))
            info_path = payload / "release-info.json"
            info = json.loads(info_path.read_text(encoding="utf-8"))
            info["signing"] = "Developer ID, notarized"
            info_path.write_text(json.dumps(info, indent=2) + "\n", encoding="utf-8")
            spec = importlib.util.spec_from_file_location("package_release", Path(__file__).with_name("package-release.py"))
            package = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(package)
            archive = directory / f"nerv-{version}-macos-arm64-portable.zip"
            package.archive_payload(payload, archive)
            authentication = ["--apple-id", os.environ["APPLE_ID"], "--password", os.environ["APPLE_APP_PASSWORD"], "--team-id", os.environ["APPLE_TEAM_ID"]]
            notarize(archive, authentication)
            unsigned = directory / f"nerv-{version}-macos-arm64-installer-unsigned.pkg"
            unsigned.unlink(missing_ok=True)
            run("python3", str(Path(__file__).with_name("build-installer.py")), "--payload", str(payload), "--output", str(directory), "--version", version, "--platform", "macos")
            signed = directory / f"nerv-{version}-macos-arm64-installer.pkg"
            run("productsign", "--keychain", str(keychain), "--sign", os.environ["MACOS_INSTALLER_IDENTITY"], "--timestamp", str(unsigned), str(signed))
            notarize(signed, authentication)
            run("xcrun", "stapler", "staple", str(signed))
            run("pkgutil", "--check-signature", str(signed))
            run("spctl", "--assess", "--type", "install", "--verbose=2", str(signed))
            unsigned.unlink()
        finally:
            run("security", "delete-keychain", str(keychain))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--payload", type=Path, required=True)
    parser.add_argument("--directory", type=Path, required=True)
    parser.add_argument("--version", required=True)
    args = parser.parse_args()
    sign(args.payload.resolve(), args.directory.resolve(), args.version)
