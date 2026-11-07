import argparse
import hashlib
import json
from pathlib import Path
import shutil
import tempfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parent.parent
MINGW_VERSION = "20261006"
MINGW_ARCHIVE = "llvm-mingw-20261006-ucrt-x86_64.zip"
MINGW_SHA256 = "317492c456aa27ee607a5919f1d2d38dcdc1112516a24d0bf4b00d078f52d17a"
DARWIN = {
    "libSystem.tbd": ("lib/libc/darwin/libSystem.tbd", "4f96f1402e1950b1764ac6accd0d2b9c1ca197b7783caae5e838fd0a4b474028"),
    "SDKSettings.json": ("lib/libc/darwin/SDKSettings.json", "7857ea6eeeac7e0e04da8d594a7b88a73a06143d171a625f107687b909d23674"),
    "LICENSE": ("LICENSE", "5c537d6853e005298a285d508cff9ac7192cea23576c840d485b2b586a7ff177"),
}


def download(url, path, checksum):
    path.parent.mkdir(parents=True, exist_ok=True)
    if not path.exists():
        with tempfile.NamedTemporaryFile(dir=path.parent, delete=False) as output:
            temporary = Path(output.name)
            try:
                with urllib.request.urlopen(url, timeout=120) as response:
                    shutil.copyfileobj(response, output)
            except Exception:
                output.close()
                temporary.unlink(missing_ok=True)
                raise
        if hashlib.sha256(temporary.read_bytes()).hexdigest() != checksum:
            temporary.unlink()
            raise RuntimeError(f"checksum mismatch: {url}")
        temporary.replace(path)
    if hashlib.sha256(path.read_bytes()).hexdigest() != checksum:
        raise RuntimeError(f"cached file checksum mismatch: {path}")
    return path


def extract(archive, destination):
    destination.mkdir(parents=True, exist_ok=False)
    with zipfile.ZipFile(archive) as bundle:
        for entry in bundle.infolist():
            parts = Path(entry.filename).parts
            relative = Path(*parts[1:])
            if not relative.parts:
                continue
            target = destination / relative
            if not target.resolve().is_relative_to(destination.resolve()):
                raise RuntimeError("toolchain archive escapes destination")
            if entry.is_dir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with bundle.open(entry) as source, target.open("wb") as output:
                    shutil.copyfileobj(source, output)


def windows(destination, cache):
    archive = download(f"https://github.com/mstorsjo/llvm-mingw/releases/download/{MINGW_VERSION}/{MINGW_ARCHIVE}", cache / MINGW_ARCHIVE, MINGW_SHA256)
    stamp = destination / ".nerv-source.json"
    if destination.exists():
        if not stamp.is_file() or json.loads(stamp.read_text())["sha256"] != MINGW_SHA256:
            raise RuntimeError("toolchain destination exists without a matching verified source stamp")
        return
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(dir=destination.parent, prefix=".nerv-toolchain-") as temporary:
        staged = Path(temporary) / "install"
        extract(archive, staged)
        for name in ["bin/x86_64-w64-mingw32-clang.exe", "bin/ld.lld.exe", "x86_64-w64-mingw32/lib/crt2.o"]:
            if not (staged / name).is_file():
                raise RuntimeError(f"toolchain archive lacks {name}")
        (staged / ".nerv-source.json").write_text(json.dumps({"version": MINGW_VERSION, "sha256": MINGW_SHA256}) + "\n")
        staged.rename(destination)


def darwin(destination, cache):
    for name, (source, checksum) in DARWIN.items():
        cached = download(f"https://raw.githubusercontent.com/ziglang/zig/0.15.2/{source}", cache / f"zig-0.15.2-{name}", checksum)
        target = destination / ("LICENSE" if name == "LICENSE" else "SDKSettings.json" if name == "SDKSettings.json" else "usr/lib/libSystem.tbd")
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(cached, target)
    for name in ["c", "m", "pthread", "dl", "util", "resolv"]:
        shutil.copy2(destination / "usr/lib/libSystem.tbd", destination / f"usr/lib/lib{name}.tbd")
    (destination / "SOURCE.json").write_text(json.dumps({"project": "ziglang/zig", "version": "0.15.2", "files": DARWIN}, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("platform", choices=["windows", "macos"])
    parser.add_argument("--destination", type=Path)
    parser.add_argument("--cache", type=Path, default=ROOT / "target/toolchains")
    args = parser.parse_args()
    destination = (args.destination or ROOT / "target/toolchains" / ("llvm-mingw" if args.platform == "windows" else "macos-sysroot")).resolve()
    if args.platform == "windows":
        windows(destination, args.cache)
    else:
        darwin(destination, args.cache)
    print(destination)


if __name__ == "__main__":
    main()
