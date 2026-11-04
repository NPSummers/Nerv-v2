#!/bin/bash
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
if [[ $# -gt 0 && "$1" == build ]]; then
    source "$root/tools/setup-llvm.sh"
    setup_llvm
    cd "$root"
    exec cargo run --quiet -p nerv -- "$@"
fi
if [[ $# -ne 1 ]]; then
    echo "Usage: $0 <file.nerv> | build <file.nerv> [-o output] [--debug|--release] [--emit-object]" >&2
    exit 1
fi
target=$1
[[ "$target" == /* ]] || target="$root/$target"
if [[ ! -f "$target" ]]; then
    echo "Nerv source file not found: $1" >&2
    exit 1
fi
if [[ "$target" != *.nerv ]]; then
    echo 'Nerv source files must use the .nerv extension' >&2
    exit 1
fi

source "$root/tools/setup-llvm.sh"
setup_llvm
cd "$root"
exec cargo run --quiet -p nerv -- "$target" --run
