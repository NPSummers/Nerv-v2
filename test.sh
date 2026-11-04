#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
if [[ $(uname -s) == Darwin ]]; then
    source ./setup-llvm.sh
    setup_llvm
fi
cargo test --workspace --locked "$@"
