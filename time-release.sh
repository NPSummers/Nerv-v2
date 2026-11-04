#!/bin/bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
program=example/heavy.nerv
runs=5
skip_build=0
show_output=0
skip_rust=0
nerv_exe=target/release/nerv
rust_exe=time-release/target/release/heavy

usage() {
    echo "Usage: $0 [--program-path FILE] [--runs N] [--skip-build] [--show-program-output] [--skip-rust-baseline] [--nerv-exe-path FILE] [--rust-exe-path FILE]"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --program-path|--runs|--nerv-exe-path|--rust-exe-path)
            if [[ $# -lt 2 ]]; then usage >&2; exit 1; fi
            case "$1" in
                --program-path) program=$2 ;;
                --runs) runs=$2 ;;
                --nerv-exe-path) nerv_exe=$2 ;;
                --rust-exe-path) rust_exe=$2 ;;
            esac
            shift 2 ;;
        --skip-build) skip_build=1; shift ;;
        --show-program-output) show_output=1; shift ;;
        --skip-rust-baseline) skip_rust=1; shift ;;
        --help|-h) usage; exit 0 ;;
        *) usage >&2; exit 1 ;;
    esac
done

if [[ ! "$runs" =~ ^[1-9][0-9]*$ ]]; then
    echo 'Runs must be >= 1.' >&2
    exit 1
fi
cd "$root"
[[ "$program" == /* ]] || program="$root/$program"
[[ "$nerv_exe" == /* ]] || nerv_exe="$root/$nerv_exe"
[[ "$rust_exe" == /* ]] || rust_exe="$root/$rust_exe"
if [[ ! -f "$program" ]]; then
    echo "Program file not found: $program" >&2
    exit 1
fi
command -v python3 >/dev/null || { echo 'Timing requires Python 3.' >&2; exit 1; }
if [[ "$skip_rust" == 0 && "$skip_build" == 0 && ! -f time-release/Cargo.toml ]]; then
    echo 'Rust baseline is missing: time-release/Cargo.toml.' >&2
    exit 1
fi
source "$root/setup-llvm.sh"
setup_llvm

if [[ "$skip_build" == 0 ]]; then
    echo 'Building release (Nerv)...'
    cargo build --quiet --release -p nerv
    if [[ "$skip_rust" == 0 ]]; then
        echo 'Building release (Rust baseline)...'
        (cd time-release && cargo build --quiet --release)
    fi
fi

python3 - "$program" "$runs" "$show_output" "$skip_rust" "$nerv_exe" "$rust_exe" <<'PY'
import os
import subprocess
import sys
import time

program, runs, show_output, skip_rust, nerv_exe, rust_exe = sys.argv[1:]
runs = int(runs)
env = os.environ.copy()
lib = os.path.join(env["LLVM_SYS_231_PREFIX"], "lib")
env["DYLD_FALLBACK_LIBRARY_PATH"] = lib + (":" + env["DYLD_FALLBACK_LIBRARY_PATH"] if env.get("DYLD_FALLBACK_LIBRARY_PATH") else "")

def display_path(path):
    relative = os.path.relpath(path)
    if relative == os.pardir or relative.startswith(os.pardir + os.sep):
        return os.path.basename(path)
    return relative

def measure(label, command):
    if not os.path.isfile(command[0]) or not os.access(command[0], os.X_OK):
        sys.exit(f"Executable not found or not executable: {command[0]}")
    print(f"\n=== {label} ===", flush=True)
    times = []
    for run in range(1, runs + 1):
        start = time.perf_counter()
        result = subprocess.run(command, env=env, stdout=None if show_output == "1" else subprocess.DEVNULL)
        elapsed = (time.perf_counter() - start) * 1000
        if result.returncode:
            sys.exit(f"{label} failed with exit code {result.returncode}.")
        times.append(elapsed)
        print(f"  Run {run}: {elapsed:.3f} ms ({elapsed / 1000:.6f} s)", flush=True)
    average = sum(times) / runs
    print(f"  Min: {min(times):.3f} ms  Avg: {average:.3f} ms  Max: {max(times):.3f} ms", flush=True)
    return average

nerv_average = measure(f"Nerv ({display_path(nerv_exe)} {display_path(program)})", [nerv_exe, program, "--run"])
if skip_rust == "1":
    sys.exit(0)
rust_average = measure(f"Rust ({display_path(rust_exe)})", [rust_exe])
print(f"\n=== Comparison (avg over {runs} runs) ===")
print(f"  Nerv : {nerv_average:.3f} ms")
print(f"  Rust : {rust_average:.3f} ms")
if rust_average > 0:
    ratio = nerv_average / rust_average
    if ratio >= 1:
        print(f"  Ratio: {ratio:.2f}x slower (Nerv / Rust)")
    elif ratio > 0:
        print(f"  Ratio: {1 / ratio:.2f}x faster (Rust / Nerv)")
PY
