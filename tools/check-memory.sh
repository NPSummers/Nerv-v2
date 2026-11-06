#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
# Experimental integer collection handles use exposed provenance. Check the safe
# core under strict provenance instead of weakening Miri's pointer checks.
export MIRIFLAGS="${MIRIFLAGS:-} -Zmiri-strict-provenance"
exec cargo +nightly miri test -p nerv-runtime --no-default-features -- --skip tests::supports_collections "$@"
