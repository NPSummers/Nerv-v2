#!/bin/bash
set -euo pipefail
root=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
if [[ $# -eq 0 ]]; then set -- --help; fi
case "$1" in
    build)
        if [[ $# -gt 1 && "$2" == *.nerv && ! -f "$2" && -f "$root/$2" ]]; then
            source_file="$root/$2"
            shift 2
            set -- build "$source_file" "$@"
        fi
        ;;
    new|run|check|test|fmt|--help|-h|--version|-V) ;;
    *)
        target=$1
        if [[ ! -f "$target" && "$target" != /* ]]; then target="$root/$target"; fi
        shift
        if [[ $# -eq 0 ]]; then set -- "$target" --run; else set -- "$target" "$@"; fi
        ;;
esac
source "$root/tools/setup-llvm.sh"
setup_llvm
exec cargo run --quiet --locked --manifest-path "$root/Cargo.toml" -p nerv -- "$@"
