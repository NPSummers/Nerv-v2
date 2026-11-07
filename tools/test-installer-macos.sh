#!/bin/bash
set -euo pipefail
[[ ${GITHUB_ACTIONS:-} == true ]] || { echo 'Run installer tests only on disposable CI hosts.' >&2; exit 1; }
payload="target/payload/nerv-$RELEASE_VERSION-macos-arm64"
package="dist/release/nerv-$RELEASE_VERSION-macos-arm64-installer.pkg"
[[ -f "$package" ]] || package="dist/release/nerv-$RELEASE_VERSION-macos-arm64-installer-unsigned.pkg"
installed="/Library/Nerv/$RELEASE_VERSION"
[[ ! -e "$installed" ]] || { echo 'Existing installation found.' >&2; exit 1; }
sudo installer -pkg "$package" -target /
trap 'sudo sh "$payload/uninstall-macos.sh" "$RELEASE_VERSION"' EXIT
diff -r "$payload" "$installed"
[[ $(readlink /usr/local/bin/nerv) == "$installed/nerv" ]]
work=$(mktemp -d)
(
    cd "$work"
    /usr/local/bin/nerv new hello
    cd hello
    env PATH= /usr/local/bin/nerv run --release --locked
)
sudo installer -pkg "$package" -target /
diff -r "$payload" "$installed"
sudo sh "$payload/uninstall-macos.sh" "$RELEASE_VERSION"
trap - EXIT
[[ ! -L /usr/local/bin/nerv && -f "$work/hello/src/main.nerv" ]]
