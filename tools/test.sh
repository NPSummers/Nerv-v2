#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ $(uname -s) == Darwin ]]; then
    source ./tools/setup-llvm.sh
    setup_llvm
fi
exec cargo run --quiet --locked -p nerv-test-runner -- "$@"
