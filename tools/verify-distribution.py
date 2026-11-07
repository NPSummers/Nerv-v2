import argparse
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import zipfile


spec = importlib.util.spec_from_file_location("package_release", Path(__file__).with_name("package-release.py"))
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


def extract(archive, destination):
    if archive.name.endswith(".tar.gz"):
        with tarfile.open(archive) as source:
            source.extractall(destination, filter="data")
    else:
        with zipfile.ZipFile(archive) as source:
            for entry in source.infolist():
                path = destination / entry.filename
                if not path.resolve().is_relative_to(destination.resolve()):
                    raise ValueError("archive member escapes destination")
                source.extract(entry, destination)
                if os.name != "nt" and entry.external_attr >> 16:
                    path.chmod((entry.external_attr >> 16) & 0o777)
    children = list(destination.iterdir())
    if len(children) != 1 or not children[0].is_dir():
        raise ValueError("archive must contain one payload directory")
    return children[0]


def verify(archive):
    with tempfile.TemporaryDirectory(prefix="nerv-verify-") as temporary:
        root = Path(temporary)
        unpacked = root / "extracted"
        unpacked.mkdir()
        payload = extract(archive, unpacked)
        moved = root / "Nerv moved \u00e9 with spaces"
        payload.rename(moved)
        binary = moved / ("nerv.exe" if os.name == "nt" else "nerv")
        work = root / "work"
        work.mkdir()
        environment = package.clean_environment()
        package.smoke(binary, work, environment)
        project = work / "smoke"
        application = project / "target/nerv/release" / ("smoke.exe" if os.name == "nt" else "smoke")
        isolated = root / "standalone"
        isolated.mkdir()
        standalone = isolated / application.name
        shutil.copy2(application, standalone)
        subprocess.run([str(standalone)], cwd=isolated, env=environment, check=True)
        if os.name != "nt":
            alias = root / "symlink-nerv"
            alias.symlink_to(binary)
            source = project / "src/main.nerv"
            subprocess.run([str(alias), "build", str(source), "-o", str(isolated / "from-symlink")], cwd=work, env=environment, check=True)
        manifest = moved / "toolchain/Nerv-toolchain.toml"
        manifest.rename(manifest.with_suffix(".disabled"))
        result = subprocess.run([str(binary), "build", str(project / "src/main.nerv"), "-o", str(isolated / "must-fail")], cwd=work, env=environment, capture_output=True, text=True)
        if result.returncode == 0 or "cannot read bundled toolchain manifest" not in result.stderr:
            raise RuntimeError(f"broken bundle did not fail explicitly: {result.stderr}")
        (moved / "toolchain").rename(moved / "disabled-toolchain")
        result = subprocess.run([str(binary), "build", str(project / "src/main.nerv"), "-o", str(isolated / "must-also-fail")], cwd=work, env=environment, capture_output=True, text=True)
        if result.returncode == 0 or "missing its bundled toolchain directory" not in result.stderr:
            raise RuntimeError(f"missing toolchain did not fail explicitly: {result.stderr}")
        print(f"Verified extracted, relocated, standalone and broken-bundle behavior: {archive.name}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("archive", type=Path)
    verify(parser.parse_args().archive.resolve(strict=True))
