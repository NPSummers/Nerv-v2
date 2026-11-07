import argparse
import importlib.util
from pathlib import Path
import subprocess
import tomllib


spec = importlib.util.spec_from_file_location("package_release", Path(__file__).with_name("package-release.py"))
package = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True, choices=package.TARGETS)
    parser.add_argument("--llvm", type=Path, default=package.ROOT / "llvm")
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--destination", type=Path, required=True)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    info = tomllib.loads(subprocess.check_output([str(args.binary.resolve()), "--print-link-info"], text=True))
    if info["target"] != args.target:
        raise ValueError("compiler target does not match linker target")
    if args.target.endswith("windows-gnullvm"):
        package.windows_toolchain(args.destination, args.source, args.llvm, args.target)
    elif args.target.endswith("apple-darwin"):
        package.mac_toolchain(args.destination, args.source, args.llvm, args.target, info["libraries"])
    else:
        package.linux_toolchain(args.destination, args.source, args.llvm, args.target, info["libraries"])


if __name__ == "__main__":
    main()
