#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
source tools/setup-llvm.sh
setup_llvm 23.1.3
version=$(python3 tools/release_version.py | python3 -c 'import json,sys; print(json.load(sys.stdin)["version"])')
if [[ $(uname -s) == Darwin ]]; then
    target=aarch64-apple-darwin
    label=macos-arm64
    python3 tools/setup-toolchain.py macos
    support="$PWD/target/toolchains/macos-sysroot"
    export MACOSX_DEPLOYMENT_TARGET=15.0
else
    target=x86_64-unknown-linux-gnu
    label=linux-x64
    support=/
fi
rustup target add "$target"
cargo fetch --locked
cargo build --locked --release --target "$target" -p nerv
binary="target/$target/release/nerv"
prepared=$(mktemp -d "$PWD/target/test-linker.XXXXXX")
python3 tools/prepare-linker.py --target "$target" --source "$support" --llvm "$LLVM_SYS_231_PREFIX" --destination "$prepared" --binary "$binary"
export NERV_TOOLCHAIN="$prepared"
cargo test --locked --workspace --target "$target"
python3 -m unittest discover -s tools -p 'test_release*.py'
payload="target/payload/nerv-$version-$label"
python3 tools/package-release.py --version "$version" --target "$target" --toolchain "$support" --llvm "$LLVM_SYS_231_PREFIX" --payload "$payload" --output dist/release
if [[ $(uname -s) == Darwin ]]; then
    python3 tools/build-installer.py --payload "$payload" --output dist/release --version "$version" --platform macos
fi
