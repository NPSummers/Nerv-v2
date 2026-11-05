#!/bin/bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
program=example/heavy.nerv
runs=5
warmups=1
skip_build=0
show_output=0
skip_rust=0
nerv_exe=target/release/heavy-nerv
rust_exe=time-release/target/release/heavy
json_output=
rustc_version=
llvm_version=
cpu=generic
effective_rustflags=

usage() {
    echo "Usage: $0 [--program-path FILE] [--runs N] [--warmups N] [--cpu generic|native] [--skip-build] [--show-program-output] [--skip-rust-baseline] [--nerv-exe-path FILE] [--rust-exe-path FILE] [--json-output FILE]"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --program-path|--runs|--warmups|--nerv-exe-path|--rust-exe-path|--json-output|--cpu)
            if [[ $# -lt 2 ]]; then usage >&2; exit 1; fi
            case "$1" in
                --program-path) program=$2 ;;
                --runs) runs=$2 ;;
                --warmups) warmups=$2 ;;
                --nerv-exe-path) nerv_exe=$2 ;;
                --rust-exe-path) rust_exe=$2 ;;
                --json-output) json_output=$2 ;;
                --cpu) cpu=$2 ;;
            esac
            shift 2 ;;
        --skip-build) skip_build=1; shift ;;
        --show-program-output) show_output=1; shift ;;
        --skip-rust-baseline) skip_rust=1; shift ;;
        --help|-h) usage; exit 0 ;;
        *) usage >&2; exit 1 ;;
    esac
done
if [[ "$cpu" != generic && "$cpu" != native ]]; then
    echo 'CPU must be generic or native.' >&2
    exit 1
fi

if [[ ! "$runs" =~ ^[1-9][0-9]*$ ]]; then
    echo 'Runs must be >= 1.' >&2
    exit 1
fi
if [[ ! "$warmups" =~ ^(0|[1-9][0-9]*)$ ]]; then
    echo 'Warmups must be >= 0.' >&2
    exit 1
fi
cd "$root"
[[ "$program" == /* ]] || program="$root/$program"
[[ "$nerv_exe" == /* ]] || nerv_exe="$root/$nerv_exe"
[[ "$rust_exe" == /* ]] || rust_exe="$root/$rust_exe"
if [[ "$skip_build" == 0 && ! -f "$program" ]]; then
    echo "Program file not found: $program" >&2
    exit 1
fi
command -v python3 >/dev/null || { echo 'Timing requires Python 3.' >&2; exit 1; }
python3 - "$nerv_exe" "$rust_exe" "$root/target/release/nerv" "$program" "$json_output" <<'PY'
from pathlib import Path
import sys
nerv, rust, compiler, source = map(lambda p: Path(p).resolve(), sys.argv[1:5])
if nerv in (rust, compiler, source):
    sys.exit('The standalone Nerv output must differ from the compiler, source, and Rust executable.')
if sys.argv[5] and Path(sys.argv[5]).resolve() in (nerv, rust, compiler, source):
    sys.exit('JSON output must not overwrite a source file, compiler, or timed executable.')
PY
if [[ "$skip_rust" == 0 && "$skip_build" == 0 && ! -f time-release/Cargo.toml ]]; then
    echo 'Rust baseline is missing: time-release/Cargo.toml.' >&2
    exit 1
fi
if [[ "$skip_build" == 0 ]]; then
    source "$root/tools/setup-llvm.sh"
    setup_llvm
    rustc_version=$("${RUSTC:-rustc}" --version)
    llvm_version=$("$LLVM_CONFIG_PATH" --version)
    echo 'Building the Nerv compiler (not timed)...'
    cargo build --quiet --release --locked -p nerv --target-dir "$root/target"
    echo 'Compiling the standalone Nerv benchmark (--release, not timed)...'
    "$root/target/release/nerv" build "$program" -o "$nerv_exe" --release --cpu "$cpu"
    if [[ "$skip_rust" == 0 ]]; then
        echo 'Building release (Rust baseline)...'
        effective_rustflags="${RUSTFLAGS:-} -C target-cpu=$cpu"
        if [[ -n "${CARGO_ENCODED_RUSTFLAGS:-}" ]]; then
            echo 'Unset CARGO_ENCODED_RUSTFLAGS for matched CPU benchmarking.' >&2
            exit 1
        fi
        RUSTFLAGS="$effective_rustflags" cargo build --quiet --release --locked --manifest-path "$root/time-release/Cargo.toml" --target-dir "$root/time-release/target"
    fi
fi

args=(--nerv-exe "$nerv_exe" --runs "$runs" --warmups "$warmups")
if [[ "$skip_build" == 0 ]]; then
    args+=(--cpu-target "$cpu" --effective-rustflags "$effective_rustflags")
fi
[[ "$skip_rust" == 1 ]] || args+=(--rust-exe "$rust_exe")
[[ "$show_output" == 0 ]] || args+=(--show-program-output)
[[ -z "$json_output" ]] || args+=(--json-output "$json_output")
[[ -z "$rustc_version" ]] || args+=(--rustc-version "$rustc_version")
[[ -z "$llvm_version" ]] || args+=(--llvm-version "$llvm_version")
[[ "$program" != "$root/example/heavy.nerv" ]] || args+=(--heavy-workload)
exec python3 "$root/tools/compare-binaries.py" "${args[@]}"
