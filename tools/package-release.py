import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import shlex
import tomllib
import subprocess
import tempfile
import zipfile
import tarfile
import re

from release_version import validate


ROOT = Path(__file__).resolve().parent.parent
TARGETS = {
    "x86_64-pc-windows-gnullvm": ("Windows", "windows-x64", "Windows 10 / UCRT", "zip"),
    "aarch64-apple-darwin": ("Darwin", "macos-arm64", "macOS 15", "zip"),
    "x86_64-unknown-linux-gnu": ("Linux", "linux-x64", "glibc 2.39 / x86-64", "tar.gz"),
}


def run(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()


def licenses(destination):
    metadata = json.loads(run("cargo", "metadata", "--locked", "--offline", "--format-version", "1"))
    for package in metadata["packages"]:
        source = Path(package["manifest_path"]).parent
        if package["source"] is None:
            continue
        target = destination / f"{package['name']}-{package['version']}"
        for path in source.iterdir():
            if path.is_file() and (path.name.upper().startswith("LICENSE") or path.name.upper().startswith("COPYING") or path.name.upper().startswith("NOTICE")):
                target.mkdir(parents=True, exist_ok=True)
                shutil.copy2(path, target / path.name)
    sysroot = Path(run("rustc", "--print", "sysroot"))
    rust_licenses = sysroot / "share/doc/rust/licenses"
    if rust_licenses.is_dir():
        shutil.copytree(rust_licenses, destination / "rust")
    for notice in (ROOT / "licenses").iterdir():
        if notice.is_file():
            shutil.copy2(notice, destination / notice.name)
    if platform.system() == "Linux":
        packages = run("dpkg-query", "-W", "-f=${binary:Package}\t${Version}\t${source:Package}\t${source:Version}\n")
        (destination / "ubuntu-build-packages.tsv").write_text(packages + "\n", encoding="utf-8")
        for path in Path("/usr/share/doc").glob("*/copyright"):
            if path.is_file():
                target = destination / "ubuntu" / path.parent.name
                target.mkdir(parents=True, exist_ok=True)
                shutil.copy2(path, target / "copyright")
        (destination / "ubuntu-sources.txt").write_text("Ubuntu 24.04 package source identities and exact versions are recorded in ubuntu-build-packages.tsv.\nMatching source is available at https://launchpad.net/ubuntu/+source/PACKAGE/VERSION and through Ubuntu deb-src repositories using apt source PACKAGE=VERSION.\nReview source redistribution obligations before publishing this bundle.\n", encoding="utf-8")


def resolve_library(dependency, original, llvm):
    if dependency.startswith(("@loader_path/", "@executable_path/")):
        candidates = [original.parent / dependency.split("/", 1)[1]]
    elif dependency.startswith("@rpath/"):
        name = dependency.removeprefix("@rpath/")
        candidates = [original.parent / name, llvm / "lib" / name]
        paths = run("otool", "-l", str(original)).splitlines()
        for index, line in enumerate(paths):
            if line.strip() == "cmd LC_RPATH":
                for value in paths[index + 1:index + 4]:
                    if value.strip().startswith("path "):
                        base = value.strip()[5:].split(" (offset", 1)[0]
                        base = base.replace("@loader_path", str(original.parent))
                        candidates.append(Path(base) / name)
        candidates.extend(sorted(llvm.rglob(Path(name).name)))
    else:
        candidates = [Path(dependency)]
    for candidate in candidates:
        if candidate.is_file():
            return candidate
    raise RuntimeError(f"cannot bundle dependency {dependency} from {original}")


def mac_libraries(binary, llvm, destination, original=None, extra=()):
    libraries = [llvm / "lib/libLLVM-23.dylib"] if (llvm / "lib/libLLVM-23.dylib").is_file() else [llvm / "lib" / name for name in run(str(llvm / "bin/llvm-config"), "--link-shared", "--libnames").split()]
    queue = [(binary, original or ROOT / "target/release/nerv"), *extra]
    for library in libraries:
        shutil.copy2(library, destination / library.name)
        queue.append((destination / library.name, library))
    visited = set()
    while queue:
        item, original = queue.pop()
        if item in visited:
            continue
        visited.add(item)
        imports = run("otool", "-L", str(item)).splitlines()[1:]
        for line in imports:
            dependency = line.strip().split(" (", 1)[0]
            if dependency.startswith(("/usr/lib/", "/System/Library/")):
                continue
            name = Path(dependency).name
            if name == item.name:
                continue
            target = destination / name
            if not target.exists():
                origin = resolve_library(dependency, original, llvm)
                shutil.copy2(origin, target)
                queue.append((target, origin))
            subprocess.run(["install_name_tool", "-change", dependency, "@loader_path/" + os.path.relpath(target, item.parent).replace(os.sep, "/"), str(item)], check=True)
        if item.suffix == ".dylib":
            subprocess.run(["install_name_tool", "-id", "@rpath/" + item.name, str(item)], check=True)
        commands = run("otool", "-l", str(item))
        for path in set(re.findall(r"cmd LC_RPATH\s+cmdsize \d+\s+path (.+?) \(offset", commands)):
            subprocess.run(["install_name_tool", "-delete_rpath", path, str(item)], check=True)
        subprocess.run(["codesign", "--force", "--sign", "-", str(item)], check=True)
        if "arm64" not in run("lipo", "-archs", str(item)).split():
            raise RuntimeError(f"non-native library in arm64 payload: {item}")
        commands = run("otool", "-l", str(item))
        versions = re.findall(r"\bminos (\d+)\.(\d+)", commands)
        versions.extend(re.findall(r"cmd LC_VERSION_MIN_MACOSX\s+cmdsize \d+\s+version (\d+)\.(\d+)", commands))
        if not versions or any((int(major), int(minor)) > (15, 0) for major, minor in versions):
            raise RuntimeError(f"{item} does not satisfy the declared macOS 15 baseline")


def toolchain_manifest(root, target, linker, arguments):
    text = f"version = 1\ntarget = {json.dumps(target)}\nlinker = {json.dumps(linker)}\narguments = {json.dumps(arguments, indent=2)}\n"
    (root / "Nerv-toolchain.toml").write_text(text, encoding="utf-8")


def windows_toolchain(root, source, llvm, target):
    source = source.resolve()
    (root / "bin").mkdir(parents=True)
    shutil.copy2(llvm / "bin/ld.lld.exe", root / "bin/ld.lld.exe")
    libraries = source / "x86_64-w64-mingw32/lib"
    destination = root / "x86_64-w64-mingw32/lib"
    destination.mkdir(parents=True)
    for item in libraries.iterdir():
        if item.suffix in (".a", ".o"):
            shutil.copy2(item, destination / item.name)
    shutil.copy2(source / "LICENSE.TXT", root / "LICENSE-LLVM-MinGW.txt")
    shutil.copytree(source / "x86_64-w64-mingw32/share/mingw32", root / "licenses/MinGW-w64")
    shutil.copy2(source / ".nerv-source.json", root / "SOURCE.json")
    with tempfile.TemporaryDirectory(prefix="nerv-link-plan-") as temporary:
        inputs = [Path(temporary) / name for name in ["program.obj", "runtime.a", "app.exe"]]
        inputs[0].touch()
        inputs[1].touch()
        command = [str(source / "bin/x86_64-w64-mingw32-clang.exe"), "-###", "-static", str(inputs[0]), str(inputs[1]), "-lNERV_NATIVE_LINK_LIBRARIES", "-o", str(inputs[2])]
        result = subprocess.run(command, capture_output=True, text=True, check=True)
        lines = [line.strip() for line in result.stderr.splitlines() if line.lstrip().startswith('"') and "ld.lld" in line]
        if len(lines) != 1:
            raise RuntimeError("could not derive the GNU linker plan from LLVM-MinGW")
        command = shlex.split(lines[0])
        paths = {str(path).replace("\\", "/"): token for path, token in zip(inputs, ["{object}", "{runtime}", "{output}"])}
        arguments = []
        prefix = source.as_posix() + "/"
        for argument in command[1:]:
            normalized = argument.replace("\\", "/")
            if normalized in paths:
                arguments.append(paths[normalized])
            elif argument == "-lNERV_NATIVE_LINK_LIBRARIES":
                arguments.append("{native-libs}")
            else:
                flag = "-L" if normalized.startswith("-L") else ""
                path = normalized[len(flag):]
                if path.startswith(prefix):
                    relative = Path(path[len(prefix):])
                    original = source / relative
                    if flag and not original.is_dir():
                        continue
                    if not flag:
                        copied = root / relative
                        if not copied.exists():
                            copied.parent.mkdir(parents=True, exist_ok=True)
                            shutil.copy2(original, copied)
                    arguments.append(flag + "{root}/" + relative.as_posix())
                elif Path(path).is_absolute():
                    raise RuntimeError(f"linker plan refers outside the bundled toolchain: {argument}")
                else:
                    arguments.append(argument)
    toolchain_manifest(root, target, "bin/ld.lld.exe", arguments)


def mac_toolchain(root, source, llvm, target, libraries):
    allowed = {"-lSystem", "-lc", "-lm", "-lpthread", "-ldl", "-lutil", "-lresolv", "-liconv"}
    unsupported = set(libraries) - allowed
    if unsupported:
        raise RuntimeError(f"macOS runtime requires additional link stubs: {sorted(unsupported)}")
    (root / "bin").mkdir(parents=True)
    shutil.copy2(llvm / "bin/ld64.lld", root / "bin/ld64.lld", follow_symlinks=True)
    shutil.copytree(source, root / "sysroot")
    architecture = "arm64" if target.startswith("aarch64") else "x86_64"
    arguments = ["-arch", architecture, "-platform_version", "macos", "15.0", "15.5", "-adhoc_codesign", "-syslibroot", "{root}/sysroot", "-L{root}/sysroot/usr/lib", "-o", "{output}", "{object}", "{runtime}", "{native-libs}"]
    toolchain_manifest(root, target, "bin/ld64.lld", arguments)


def linux_toolchain(root, source, llvm, target, libraries):
    libc, version = platform.libc_ver()
    if libc != "glibc" or tuple(map(int, version.split(".")[:2])) != (2, 39):
        raise RuntimeError("produce the initial Linux bundle on the declared glibc 2.39 baseline")
    allowed = {"-lgcc_s", "-lgcc_eh", "-lutil", "-lrt", "-lpthread", "-lm", "-ldl", "-lc", "-lgcc", "-lresolv"}
    if set(libraries) - allowed:
        raise RuntimeError(f"unsupported Linux runtime libraries: {libraries}")
    if "-lgcc_s" in libraries:
        raise RuntimeError("rebuild the compiler with static unwind link metadata")
    (root / "bin").mkdir(parents=True)
    shutil.copy2(llvm / "bin/ld.lld", root / "bin/ld.lld", follow_symlinks=True)
    destination = root / "lib"
    destination.mkdir()
    names = ["Scrt1.o", "crti.o", "crtn.o", "crtbeginS.o", "crtendS.o", "libc.so.6", "libc_nonshared.a", "libm.so.6", "libmvec.so.1", "libpthread.a", "libdl.a", "libutil.a", "librt.a", "libresolv.so.2", "libgcc.a", "libgcc_eh.a", "ld-linux-x86-64.so.2"]
    for name in names:
        path = Path(run("gcc", f"-print-file-name={name}"))
        if not path.is_absolute() or not path.is_file():
            raise RuntimeError(f"cannot locate Linux linker support file {name}")
        shutil.copy2(path, destination / name, follow_symlinks=True)
    for name, contents in {"libc.so": "GROUP ( libc.so.6 libc_nonshared.a AS_NEEDED ( ld-linux-x86-64.so.2 ) )", "libm.so": "GROUP ( libm.so.6 AS_NEEDED ( libmvec.so.1 ) )", "libresolv.so": "INPUT ( libresolv.so.2 )"}.items():
        (destination / name).write_text(contents + "\n", encoding="utf-8")
    arguments = ["-pie", "--eh-frame-hdr", "-m", "elf_x86_64", "--dynamic-linker", "/lib64/ld-linux-x86-64.so.2", "-o", "{output}", "{root}/lib/Scrt1.o", "{root}/lib/crti.o", "{root}/lib/crtbeginS.o", "-L{root}/lib", "{object}", "{runtime}", "--start-group", "{native-libs}", "-lgcc", "-lgcc_eh", "--end-group", "{root}/lib/crtendS.o", "{root}/lib/crtn.o"]
    toolchain_manifest(root, target, "bin/ld.lld", arguments)


def elf_libraries(binary, llvm, destination, original, linker):
    config = llvm / "bin/llvm-config"
    names = ["libLLVM-23.so"] if (llvm / "lib/libLLVM-23.so").is_file() else run(str(config), "--link-shared", "--libnames").split()
    for name in names:
        library = destination / name
        shutil.copy2(llvm / "lib" / name, library, follow_symlinks=True)
        soname = run("patchelf", "--print-soname", str(library))
        if soname and soname != name:
            (destination / soname).symlink_to(name)
    queue = [binary, linker, *(destination / name for name in names)]
    visited = set()
    system = {"libc.so.6", "libm.so.6", "libpthread.so.0", "libdl.so.2", "librt.so.1", "libresolv.so.2", "libutil.so.1", "ld-linux-x86-64.so.2"}
    while queue:
        item = queue.pop()
        if item in visited:
            continue
        visited.add(item)
        for line in run("ldd", str(item)).splitlines():
            if "not found" in line:
                raise RuntimeError(f"unresolved dependency in {item}: {line}")
            if "=>" not in line:
                continue
            name, path = line.strip().split(" => ", 1)
            if name in system:
                continue
            target = destination / name
            if not target.exists():
                shutil.copy2(path.split(" (", 1)[0], target, follow_symlinks=True)
                queue.append(target)
        subprocess.run(["patchelf", "--set-rpath", "$ORIGIN/" + os.path.relpath(destination, item.parent), str(item)], check=True)
        versions = re.findall(r"GLIBC_(\d+)\.(\d+)", run("readelf", "--version-info", str(item)))
        if any((int(major), int(minor)) > (2, 39) for major, minor in versions):
            raise RuntimeError(f"{item} exceeds the declared glibc 2.39 baseline")
        notes = run("readelf", "--notes", str(item))
        if re.search(r"x86 ISA needed:.*x86-64-v[234]", notes):
            raise RuntimeError(f"{item} requires a newer x86-64 ISA than the release baseline")


def clean_environment():
    environment = dict(os.environ)
    for key in list(environment):
        if key.upper() in {"PATH", "LIB", "LIBPATH", "INCLUDE", "NERV_LINKER", "NERV_TOOLCHAIN", "CC", "CXX", "SDKROOT", "DEVELOPER_DIR", "CPATH", "LIBRARY_PATH", "C_INCLUDE_PATH", "CPLUS_INCLUDE_PATH", "LLVM_SYS_231_PREFIX", "LLVM_CONFIG_PATH", "DYLD_LIBRARY_PATH", "DYLD_FALLBACK_LIBRARY_PATH", "LD_LIBRARY_PATH", "CARGO_HOME", "RUSTUP_HOME"}:
            environment.pop(key, None)
    environment["PATH"] = ""
    return environment


def windows_dependencies(bundle, llvm):
    system = {"kernel32.dll", "ntdll.dll", "advapi32.dll", "bcrypt.dll", "bcryptprimitives.dll", "ws2_32.dll", "userenv.dll", "dbghelp.dll", "ole32.dll", "oleaut32.dll", "shell32.dll", "user32.dll", "gdi32.dll", "version.dll", "shlwapi.dll", "psapi.dll", "secur32.dll", "crypt32.dll", "rpcrt4.dll", "ucrtbase.dll", "msvcrt.dll"}
    for path in [*bundle.glob("*.exe"), *bundle.glob("*.dll"), *bundle.glob("toolchain/bin/*.exe")]:
        imports = run(str(llvm / "bin/llvm-readobj.exe"), "--coff-imports", str(path))
        for name in re.findall(r"^\s*Name: (.+\.dll)$", imports, re.MULTILINE | re.IGNORECASE):
            if name.lower() not in system and not name.lower().startswith(("api-ms-win-", "ext-ms-win-")) and not (bundle / name).is_file():
                raise RuntimeError(f"unbundled DLL dependency in {path.name}: {name}")


def smoke(binary, directory, environment):
    subprocess.run([str(binary), "--version"], cwd=directory, env=environment, check=True)
    subprocess.run([str(binary), "new", "smoke"], cwd=directory, env=environment, check=True)
    project = directory / "smoke"
    subprocess.run([str(binary), "check", "--locked"], cwd=project, env=environment, check=True)
    for arguments in [["run", "--locked"], ["run", "--release", "--locked"], ["test", "--locked"]]:
        subprocess.run([str(binary), *arguments], cwd=project, env=environment, check=True)
    (project / "src/main.nerv").write_text('import nerv.std\nfn square(value: i64) -> i64 => value * value\nfn main() -> i64\n    let task = result_or(spawn_task(6, square), Task { id: -1 })\n    assert(result_or(join_task(task), -1) == 36)\n    let parts = split_text("native,toolchain", ",")\n    assert(parts[1] == "toolchain")\n    assert("Nerv" == nerv.string.concat("Ne", "rv"))\n    0\n', encoding="utf-8")
    subprocess.run([str(binary), "run", "--release", "--locked"], cwd=project, env=environment, check=True)


def archive_payload(bundle, archive):
    if archive.name.endswith(".tar.gz"):
        with tarfile.open(archive, "w:gz") as output:
            output.add(bundle, arcname=bundle.name)
    else:
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9, strict_timestamps=False) as output:
            for path in sorted(bundle.rglob("*")):
                if path.is_file():
                    output.write(path, path.relative_to(bundle.parent))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", required=True)
    parser.add_argument("--llvm", type=Path, default=ROOT / "llvm")
    parser.add_argument("--output", type=Path, default=ROOT / "dist")
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--toolchain", type=Path, required=True)
    parser.add_argument("--payload", type=Path)
    args = parser.parse_args()
    version = validate()["version"]
    if version != args.version:
        raise RuntimeError(f"package version {version} does not match {args.version}")
    if args.target not in TARGETS:
        raise RuntimeError(f"unsupported release target: {args.target}")
    host, label, baseline, extension = TARGETS[args.target]
    windows = args.target.endswith("windows-gnullvm")
    if host != platform.system():
        raise RuntimeError("package on the native host")
    llvm_version = run(str(args.llvm / "bin" / ("llvm-config.exe" if windows else "llvm-config")), "--version")
    if llvm_version != "23.1.3":
        raise RuntimeError(f"release packaging requires pinned LLVM 23.1.3, got {llvm_version}")
    name = f"nerv-{args.version}-{label}"
    args.output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="nerv-package-") as temporary:
        bundle = Path(temporary) / name
        bundle.mkdir()
        binary_name = "nerv.exe" if windows else "nerv"
        binary = bundle / binary_name
        original = args.binary or ROOT / "target" / args.target / "release" / binary_name
        shutil.copy2(original, binary)
        info = tomllib.loads(subprocess.check_output([str(original.resolve()), "--print-link-info"], text=True))
        if info["target"] != args.target:
            raise RuntimeError("compiler target does not match release target")
        toolchain = bundle / "toolchain"
        if windows:
            shutil.copy2(args.llvm / "bin/LLVM-C.dll", bundle / "LLVM-C.dll")
            shutil.copy2(args.toolchain / "bin/libunwind.dll", bundle / "libunwind.dll")
            windows_toolchain(toolchain, args.toolchain, args.llvm, args.target)
            windows_dependencies(bundle, args.llvm)
        elif host == "Darwin":
            mac_toolchain(toolchain, args.toolchain, args.llvm, args.target, info["libraries"])
            mac_libraries(binary, args.llvm, bundle, original, [(toolchain / "bin/ld64.lld", args.llvm / "bin/ld64.lld")])
            shutil.copy2(ROOT / "tools/installers/uninstall-macos.sh", bundle / "uninstall-macos.sh")
        else:
            linux_toolchain(toolchain, args.toolchain, args.llvm, args.target, info["libraries"])
            elf_libraries(binary, args.llvm, bundle, original, toolchain / "bin/ld.lld")
        shutil.copytree(ROOT / "docs", bundle / "docs")
        shutil.copy2(ROOT / "LICENSE", bundle / "LICENSE")
        shutil.copy2(ROOT / "docs/installation.md", bundle / "README.md")
        (bundle / "licenses").mkdir()
        licenses(bundle / "licenses")
        (bundle / "VERSION").write_text(f"{args.version}\n{args.target}\n{run('git', 'rev-parse', 'HEAD')}\n", encoding="utf-8")
        manifest = {"version": args.version, "source_commit": run("git", "rev-parse", "HEAD"), "source_dirty": bool(run("git", "status", "--porcelain")), "target": args.target, "platform": label, "minimum_system": baseline, "llvm": llvm_version, "rust": run("rustc", "--version"), "signing": "unsigned", "toolchain": "LLVM-MinGW 20261006" if windows else "Zig 0.15.2 Darwin stubs" if host == "Darwin" else "Ubuntu 24.04 glibc 2.39 link files"}
        (bundle / "release-info.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
        if host == "Darwin":
            files = [path.relative_to(bundle).as_posix() for path in sorted(bundle.rglob("*")) if path.is_file()]
            (bundle / ".nerv-files").write_text("\n".join(files + [".nerv-files"]) + "\n", encoding="utf-8")
        environment = clean_environment()
        smoke(binary, Path(temporary), environment)
        relocated = Path(temporary) / "relocated space \u00e9"
        bundle.rename(relocated)
        bundle = relocated
        relocation = Path(temporary) / "relocation"
        relocation.mkdir()
        smoke(bundle / binary_name, relocation, environment)
        bundle = bundle.rename(Path(temporary) / name)
        archive = args.output / f"{name}-portable.{extension}"
        archive_payload(bundle, archive)
        if args.payload:
            shutil.copytree(bundle, args.payload, symlinks=True)
    checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
    Path(str(archive) + ".sha256").write_text(f"{checksum}  {archive.name}\n", encoding="ascii")
    print(archive)


if __name__ == "__main__":
    main()
