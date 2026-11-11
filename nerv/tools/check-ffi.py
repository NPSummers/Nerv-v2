import argparse
import os
from pathlib import Path
import subprocess


ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--compiler", required=True, type=Path)
    parser.add_argument("--cc", required=True, type=Path)
    args = parser.parse_args()
    compiler = args.compiler.resolve(strict=True)
    cc = args.cc.resolve(strict=True)
    directory = ROOT / "target/ffi-cases"
    directory.mkdir(exist_ok=True)
    source = directory / "native.c"
    source.write_text("""#include <stdint.h>
#if defined(_WIN32)
#define API __declspec(dllexport)
#else
#define API __attribute__((visibility("default")))
#endif
API int32_t signed_value(int32_t x) { return x - 7; }
API uint32_t unsigned_value(uint32_t x) { return x ^ 0xa0000000u; }
API float float_value(float x, float y) { return x + y; }
API double wide_value(double x, int32_t y, double z) { return x + y + z; }
API void poke(int64_t *address, int64_t value) { *address = value; }
API int64_t many(int64_t a,int64_t b,int64_t c,int64_t d,int64_t e,int64_t f,int64_t g,int64_t h) { return a+b+c+d+e+f+g+h; }
""", encoding="utf-8")
    library = directory / ("native.dll" if os.name == "nt" else "native.so")
    subprocess.run([str(cc), "-shared", "-O2", str(source), "-o", str(library)], check=True)
    file = directory / "main.nerv"
    file.write_text("""import nerv.std
fn main()
    let library = nerv.ffi.open(option_or(environment("NERV_FFI_TEST_LIBRARY"), ""))
    assert(library != 0)
    assert(nerv.ffi.symbol(library, "absent") == 0)
    assert(nerv.ffi.call_i32(nerv.ffi.symbol(library, "signed_value"), i32(-35)) == i32(-42))
    assert(nerv.ffi.call_u32(nerv.ffi.symbol(library, "unsigned_value"), u32(2684354561)) == u32(1))
    assert(nerv.ffi.call_f32(nerv.ffi.symbol(library, "float_value"), f32(1.25), f32(2.5)) == f32(3.75))
    assert(nerv.ffi.call_f64(nerv.ffi.symbol(library, "wide_value"), 1.25, i32(40), 0.75) == 42.0)
    assert(nerv.ffi.call_i64(nerv.ffi.symbol(library, "many"), 1, 2, 3, 4, 5, 6, 7, 14) == 42)
    let memory = nerv.ffi.alloc(16)
    nerv.ffi.call_void(nerv.ffi.symbol(library, "poke"), memory, 42)
    assert(nerv.ffi.load(memory) == 42)
    nerv.ffi.store(memory + 8, -42)
    assert(nerv.ffi.load(memory + 8) == -42)
    nerv.ffi.store_byte(memory, 255)
    assert(nerv.ffi.load_byte(memory) == 255)
    nerv.ffi.free(memory)
    let name = nerv.ffi.cstring("native")
    assert(nerv.ffi.text(name) == "native")
    nerv.ffi.free(name)
    nerv.ffi.close(library)
""", encoding="utf-8")
    env = os.environ.copy()
    env["NERV_FFI_TEST_LIBRARY"] = str(library)
    for mode in ["--debug", "--release"]:
        output = directory / ("ffi.exe" if os.name == "nt" else "ffi")
        subprocess.run([str(compiler), "build", str(file), mode, "-o", str(output)], cwd=ROOT, env=env, check=True)
        subprocess.run([str(output)], cwd=ROOT, env=env, check=True)
    print("Native FFI passed: signed/unsigned widths, floats, mixed registers, stack arguments, pointers, void calls, allocation/string ownership")


if __name__ == "__main__":
    main()
