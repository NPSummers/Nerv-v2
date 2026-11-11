import argparse
import hashlib
from pathlib import Path
import subprocess
import runpy


ROOT = Path(__file__).resolve().parent.parent
validate_ir = runpy.run_path(str(Path(__file__).with_name("check-sema.py")))["validate_ir"]


def manifest(name, dependencies="", fields=""):
    return f'[package]\nname = "{name}"\nversion = "0.1.0"\n{fields}\n[dependencies]\n{dependencies}'


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--stage0", type=Path, required=True)
    parser.add_argument("--compiler", type=Path, required=True)
    args = parser.parse_args()
    stage0 = args.stage0.resolve(strict=True)
    compiler = args.compiler.resolve(strict=True)
    basic = {
        "app/Nerv.toml": manifest("app", 'helper = { path = "../lib" }\n'),
        "app/src/main.nerv": "import helper\nfn main() -> i64 => helper.value()\n",
        "lib/Nerv.toml": manifest("library"),
        "lib/src/lib.nerv": "fn value() -> i64 => 42\n",
    }
    cases = [(True, basic)]
    custom = dict(basic)
    custom["lib/Nerv.toml"] = manifest("library", fields='lib = "code/api.nerv"\n')
    custom.pop("lib/src/lib.nerv")
    custom["lib/code/api.nerv"] = "fn value() -> i64 => 42\n"
    cases.append((True, custom))
    suffix = dict(custom)
    suffix["app/src/main.nerv"] = "import helper.extra\nfn main() -> i64 => helper.extra.value()\n"
    suffix["lib/code/extra.nerv"] = "fn value() -> i64 => 42\n"
    cases.append((True, suffix))
    nested = dict(basic)
    nested["lib/Nerv.toml"] = manifest("library", 'helper = { path = "../leaf" }\n')
    nested["lib/src/lib.nerv"] = "import helper\nfn value() -> i64 => helper.number()\n"
    nested["leaf/Nerv.toml"] = manifest("leaf")
    nested["leaf/src/lib.nerv"] = "fn number() -> i64 => 42\n"
    cases.append((True, nested))
    diamond = dict(nested)
    diamond["app/Nerv.toml"] = manifest("app", 'helper = { path = "../lib" }\nleaf = { path = "../leaf" }\n')
    diamond["app/src/main.nerv"] = "import helper\nimport leaf\nfn main() -> i64 => helper.value() + leaf.number()\n"
    cases.append((True, diamond))
    own = dict(basic)
    own["app/src/main.nerv"] = "import app\nfn main() -> i64 => app.number()\n"
    own["app/src/lib.nerv"] = "fn number() -> i64 => 42\n"
    cases.append((True, own))
    fallback = dict(basic)
    fallback["lib/src/lib.nerv"] = "import nested.child\nfn value() -> i64 => nested.child.value()\n"
    fallback["lib/src/nested/child.nerv"] = "import shared\nfn value() -> i64 => shared.number()\n"
    fallback["lib/src/shared.nerv"] = "fn number() -> i64 => 42\n"
    cases.append((True, fallback))
    alias = dict(basic)
    alias["app/src/helper.nerv"] = "fn value() -> bool => true\n"
    cases.append((True, alias))
    missing = dict(basic)
    missing.pop("lib/src/lib.nerv")
    cases.append((False, missing))
    cycle = dict(basic)
    cycle["lib/Nerv.toml"] = manifest("library", 'app = { path = "../app" }\n')
    cases.append((False, cycle))
    for text in [
        manifest("nerv"), manifest("app", 'nerv = { path = "../lib" }\n'),
        manifest("app", 'app = { path = "../lib" }\n'),
        manifest("app", 'helper = "1.0.0"\n'),
        manifest("app", 'helper = { git = "https://example.invalid" }\n'),
        manifest("app", 'helper = {}\n'),
        manifest("app", fields='entry = "../main.nerv"\n'),
        manifest("app", fields='entry = "main.txt"\n'),
        manifest("app", fields='unknown = "value"\n'),
        manifest("app").replace('0.1.0', '1.0'),
        manifest("app").replace('0.1.0', '18446744073709551616.0.0'),
        'package = { name = "app", version = "1.0.0" }\ndependencies = { helper = {} }\ndependencies.helper.path = "../lib"\n',
        'package.name = "app"\npackage.version = "1.0.0"\n[package]\n',
        manifest("app") + '[other]\nvalue = "bad"\n',
        manifest("app") + '[package]\nname = "again"\n',
    ]:
        files = dict(basic)
        files["app/Nerv.toml"] = text
        cases.append((False, files))
    unicode_case = dict(basic)
    unicode_case["app/Nerv.toml"] = manifest("app", 'helper = { path = "../caf\u00e9 space" }\n')
    unicode_case["caf\u00e9 space/Nerv.toml"] = unicode_case.pop("lib/Nerv.toml")
    unicode_case["caf\u00e9 space/src/lib.nerv"] = unicode_case.pop("lib/src/lib.nerv")
    cases.append((True, unicode_case))
    library = {
        "app/Nerv.toml": manifest("app"),
        "app/src/lib.nerv": "fn value() -> i64 => 42\n",
    }
    cases.append((True, library))
    dotted = dict(basic)
    dotted["app/Nerv.toml"] = 'package.name = "app"\npackage.version = "0.1.0"\ndependencies.helper.path = "../lib"\n'
    cases.append((True, dotted))
    table = dict(basic)
    table["app/Nerv.toml"] = manifest("app") + '[dependencies.helper]\npath = "../lib"\n'
    cases.append((True, table))
    for index, (accepted, files) in enumerate(cases):
        directory = ROOT / "target/project-cases" / f"case_{index}"
        for name, text in files.items():
            file = directory / name
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text(text, encoding="utf-8", newline="")
        package = directory / "app"
        lock = package / "Nerv.lock"
        lock.unlink(missing_ok=True)
        def run(executable, *arguments):
            return subprocess.run([str(executable), *arguments], cwd=package, capture_output=True, text=True, encoding="utf-8", timeout=30)
        old = run(stage0, "check")
        expected = 0 if accepted else 1
        if old.returncode != expected:
            raise RuntimeError(f"reference case {index}: {old.stderr}")
        reference = lock.read_bytes() if lock.exists() else None
        lock.unlink(missing_ok=True)
        if accepted:
            absent = run(compiler, "--check", "--locked")
            if absent.returncode != 1 or lock.exists():
                raise RuntimeError(f"missing locked case {index}: {absent.stderr}")
        new = run(compiler, "--check", "--manifest-path", "Nerv.toml")
        if new.returncode != expected:
            raise RuntimeError(f"new case {index}: {new.stderr}")
        if not accepted:
            if lock.exists() or new.stdout:
                raise RuntimeError(f"failed graph produced output/lock: {index}")
            continue
        actual = lock.read_bytes()
        if actual != reference:
            raise RuntimeError(f"lock mismatch case {index}:\n{reference!r}\n{actual!r}")
        for name, text in files.items():
            if name.endswith("Nerv.toml"):
                digest = hashlib.sha256(text.encode()).hexdigest().encode()
                if digest not in actual:
                    raise RuntimeError(f"manifest hash missing: {name}")
        for executable, arguments in [(stage0, ("check", "--locked")), (compiler, ("--check", "--locked"))]:
            checked = run(executable, *arguments)
            if checked.returncode:
                raise RuntimeError(f"locked case {index}: {checked.stderr}")
        source_name = "app/src/main.nerv" if "app/src/main.nerv" in files else "app/src/lib.nerv"
        entry = directory / source_name
        dumped = run(compiler, "--ir", "--locked")
        if dumped.returncode:
            raise RuntimeError(f"project IR case {index}: {dumped.stderr}")
        validate_ir(dumped.stdout, entry.read_bytes())
        elsewhere = subprocess.run([str(compiler), "--check", "--manifest-path", str(package / "Nerv.toml"), "--locked"], cwd=ROOT, capture_output=True, text=True, encoding="utf-8", timeout=30)
        if elsewhere.returncode:
            raise RuntimeError(f"explicit manifest from another directory {index}: {elsewhere.stderr}")
        entry.write_text(files[source_name] + "\n", encoding="utf-8")
        if run(compiler, "--check", "--locked").returncode or lock.read_bytes() != actual:
            raise RuntimeError(f"source edit invalidated lock: {index}")
        config = package / "Nerv.toml"
        config.write_text(files["app/Nerv.toml"] + "\n", encoding="utf-8")
        stale = run(compiler, "--check", "--locked")
        if stale.returncode != 1 or lock.read_bytes() != actual:
            raise RuntimeError(f"stale lock changed: {index}")
        if run(compiler, "--check").returncode or lock.read_bytes() == actual:
            raise RuntimeError(f"lock refresh failed: {index}")
        if run(stage0, "check", "--locked").returncode:
            raise RuntimeError(f"refreshed lock incompatible: {index}")
    print(f"Project checks passed: {len(cases)} cases; compatible locks, hashes, missing/stale locks, source edits")


if __name__ == "__main__":
    main()
