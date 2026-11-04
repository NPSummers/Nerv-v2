#!/bin/bash

prepare_llvm() {
    local config=$1 prefix=$2 sdk library output
    local libraries=() system_libs=()
    if [[ -f "$prefix/lib/libLLVM-23.dylib" ]] || "$config" --link-shared --libnames >/dev/null 2>&1; then
        return 0
    fi

    if [[ ! -x "$prefix/bin/clang++" || ! -x "$prefix/bin/ld64.lld" ]]; then
        echo 'Static LLVM needs clang++ and ld64.lld to prepare its shared library.' >&2
        return 1
    fi
    read -r -a libraries <<< "$("$config" --link-static --libnames core orcjit native passes)"
    read -r -a system_libs <<< "$("$config" --link-static --system-libs)"
    for library in "${system_libs[@]}"; do
        if [[ "$library" == /* && ! -f "$library" ]]; then
            echo "LLVM requires $library. Install zstd with 'brew install zstd', then retry." >&2
            return 1
        fi
    done
    for library in "${!libraries[@]}"; do
        libraries[$library]="-Wl,-force_load,$prefix/lib/${libraries[$library]}"
    done
    sdk=$(xcrun --show-sdk-path) || return 1
    output="$prefix/lib/.libLLVM-23.dylib.tmp"
    echo 'Preparing the LLVM shared library (first run only)...' >&2
    if ! "$prefix/bin/clang++" -dynamiclib -fuse-ld=lld -isysroot "$sdk" \
        -Wl,-install_name,@rpath/libLLVM-23.dylib "${libraries[@]}" \
        "${system_libs[@]}" -o "$output"; then
        rm -f "$output"
        return 1
    fi
    mv "$output" "$prefix/lib/libLLVM-23.dylib"
}

setup_llvm() {
    local root config prefix version archive checksum staging
    root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
    if [[ $(uname -s) != Darwin ]]; then
        echo 'This script requires macOS.' >&2
        return 1
    fi

    for config in "${LLVM_CONFIG_PATH:-}" "${LLVM_SYS_231_PREFIX:-}/bin/llvm-config" \
        "$root/llvm/bin/llvm-config" "$(command -v llvm-config || true)" \
        /opt/homebrew/opt/llvm/bin/llvm-config /usr/local/opt/llvm/bin/llvm-config; do
        [[ -x "$config" ]] || continue
        version=$("$config" --version) || continue
        [[ "$version" == 23.1.* ]] || continue
        prefix=$("$config" --prefix) || continue
        [[ -f "$prefix/lib/libLLVM.dylib" || -f "$prefix/lib/libLLVM-23.dylib" || -f "$prefix/lib/libLLVMCore.a" ]] || continue
        prepare_llvm "$config" "$prefix" || return 1
        export LLVM_CONFIG_PATH="$config"
        export LLVM_SYS_231_PREFIX="$prefix"
        export PATH="$prefix/bin:$PATH"
        export DYLD_FALLBACK_LIBRARY_PATH="$prefix/lib${DYLD_FALLBACK_LIBRARY_PATH:+:$DYLD_FALLBACK_LIBRARY_PATH}"
        return 0
    done

    if [[ $(uname -m) != arm64 ]]; then
        echo 'LLVM 23.1.3 has no official Intel macOS archive. Set LLVM_SYS_231_PREFIX to a compatible LLVM 23.1 installation.' >&2
        return 1
    fi
    if [[ -e "$root/llvm" ]]; then
        echo 'llvm/ exists but is not a usable LLVM 23.1 installation. Move it aside and retry.' >&2
        return 1
    fi

    version=23.1.3
    archive="LLVM-$version-macOS-ARM64.tar.xz"
    checksum=3dcfd88aec10be204a24037bcb0dc0ad777882e9dbe8ac60e2639060d8a794b1
    echo "Downloading LLVM $version into $root/llvm..." >&2
    (
        set -e
        staging=$(mktemp -d "$root/.llvm-setup.XXXXXX") || exit 1
        trap 'rm -rf "$staging"' EXIT
        curl --fail --location --retry 3 --output "$staging/$archive" \
            "https://github.com/llvm/llvm-project/releases/download/llvmorg-$version/$archive" || exit 1
        printf '%s  %s\n' "$checksum" "$staging/$archive" | shasum -a 256 -c - || exit 1
        mkdir "$staging/install" || exit 1
        tar -xf "$staging/$archive" -C "$staging/install" --strip-components=1 || exit 1
        [[ -x "$staging/install/bin/llvm-config" && -f "$staging/install/lib/libLLVMCore.a" ]] || exit 1
        mv "$staging/install" "$root/llvm" || exit 1
    ) || return 1
    setup_llvm
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    set -euo pipefail
    setup_llvm
fi
