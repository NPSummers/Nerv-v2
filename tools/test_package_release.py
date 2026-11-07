from pathlib import Path
import tempfile
import tomllib
import unittest
from unittest.mock import patch

import importlib.util
spec = importlib.util.spec_from_file_location("package_release", Path(__file__).with_name("package-release.py"))
package_release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package_release)


class LibraryResolution(unittest.TestCase):
    def test_resolves_rpath_libraries_from_llvm_subdirectories(self):
        with tempfile.TemporaryDirectory() as directory:
            llvm = Path(directory)
            library = llvm / "lib/aarch64-apple-darwin/libunwind.1.dylib"
            library.parent.mkdir(parents=True)
            library.write_bytes(b"library")
            with patch.object(package_release, "run", return_value=""):
                actual = package_release.resolve_library("@rpath/libunwind.1.dylib", llvm / "lib/libLLVM-23.dylib", llvm)
            self.assertEqual(actual, library)

    def test_resolves_loader_relative_paths(self):
        with tempfile.TemporaryDirectory() as directory:
            llvm = Path(directory)
            library = llvm / "libunwind.1.dylib"
            library.write_bytes(b"library")
            actual = package_release.resolve_library("@loader_path/libunwind.1.dylib", llvm / "libLLVM-23.dylib", llvm)
            self.assertEqual(actual, library)

    def test_uses_original_macho_rpaths_and_reports_missing_libraries(self):
        with tempfile.TemporaryDirectory() as directory:
            llvm = Path(directory)
            library = llvm / "vendor/libunwind.1.dylib"
            library.parent.mkdir()
            library.write_bytes(b"library")
            output = "cmd LC_RPATH\ncmdsize 48\npath @loader_path/vendor (offset 12)\n"
            with patch.object(package_release, "run", return_value=output):
                actual = package_release.resolve_library("@rpath/libunwind.1.dylib", llvm / "libLLVM.dylib", llvm)
                self.assertEqual(actual, library)
                with self.assertRaisesRegex(RuntimeError, "cannot bundle dependency"):
                    package_release.resolve_library("@rpath/missing.dylib", llvm / "libLLVM.dylib", llvm)


class Toolchains(unittest.TestCase):
    def test_linux_link_plan_is_relocatable_and_has_exactly_one_input_slot(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            producer = root / "producer"
            producer.mkdir()
            llvm = root / "llvm"
            (llvm / "bin").mkdir(parents=True)
            (llvm / "bin/ld.lld").write_bytes(b"linker")
            def run(command, argument):
                path = producer / argument.split("=", 1)[1]
                path.write_bytes(b"support")
                return str(path.resolve())
            destination = root / "bundle/toolchain"
            with patch.object(package_release.platform, "libc_ver", return_value=("glibc", "2.39")), patch.object(package_release, "run", side_effect=run):
                package_release.linux_toolchain(destination, Path("/"), llvm, "x86_64-unknown-linux-gnu", ["-lgcc_eh", "-lc", "-lm", "-lpthread"])
            source = (destination / "Nerv-toolchain.toml").read_text(encoding="utf-8")
            self.assertNotIn(str(producer), source)
            manifest = tomllib.loads(source)
            for placeholder in ["{object}", "{runtime}", "{output}", "{native-libs}"]:
                self.assertEqual(manifest["arguments"].count(placeholder), 1)
            self.assertEqual((destination / "lib/libc.so").read_text().strip(), "GROUP ( libc.so.6 libc_nonshared.a AS_NEEDED ( ld-linux-x86-64.so.2 ) )")

    def test_linux_rejects_unverified_baselines_and_dynamic_unwind_metadata(self):
        with patch.object(package_release.platform, "libc_ver", return_value=("glibc", "2.40")):
            with self.assertRaisesRegex(RuntimeError, "glibc 2.39"):
                package_release.linux_toolchain(Path("unused"), Path("/"), Path("llvm"), "target", [])
        with patch.object(package_release.platform, "libc_ver", return_value=("glibc", "2.39")):
            with self.assertRaisesRegex(RuntimeError, "static unwind"):
                package_release.linux_toolchain(Path("unused"), Path("/"), Path("llvm"), "target", ["-lgcc_s"])

    def test_unbundled_windows_dlls_are_reported(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "nerv.exe").write_bytes(b"program")
            with patch.object(package_release, "run", return_value="Import {\n  Name: VCRUNTIME140.dll\n}\n"):
                with self.assertRaisesRegex(RuntimeError, "VCRUNTIME140.dll"):
                    package_release.windows_dependencies(root, Path("llvm"))
                (root / "VCRUNTIME140.dll").write_bytes(b"library")
                package_release.windows_dependencies(root, Path("llvm"))


if __name__ == "__main__":
    unittest.main()
