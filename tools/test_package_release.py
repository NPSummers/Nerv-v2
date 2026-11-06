from pathlib import Path
import tempfile
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


if __name__ == "__main__":
    unittest.main()
