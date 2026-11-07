#!/bin/sh
set -eu
if [ "$(id -u)" != 0 ] || [ "$#" != 1 ]; then
    echo 'Usage: sudo sh uninstall-macos.sh VERSION' >&2
    exit 1
fi
version=$1
case "$version" in ''|*[!0-9A-Za-z.+-]*) echo 'Invalid version' >&2; exit 1 ;; esac
payload="/Library/Nerv/$version"
if [ -L "$payload" ] || [ ! -f "$payload/.nerv-files" ] || [ ! -f "$payload/VERSION" ] || [ "$(head -n 1 "$payload/VERSION")" != "$version" ]; then
    echo 'No matching Nerv payload found' >&2
    exit 1
fi
resolved=$(cd "$payload" && pwd -P)
[ "$resolved" = "$payload" ] || { echo 'Installation path resolves outside its expected location' >&2; exit 1; }
if [ -L /usr/local/bin/nerv ] && [ "$(readlink /usr/local/bin/nerv)" = "$payload/nerv" ]; then
    rm /usr/local/bin/nerv
fi
identifier=$(printf '%s' "$version" | od -An -tx1 | tr -d ' \n')
pkgutil --forget "org.nerv.compiler.v$identifier"
while IFS= read -r relative; do
    case "$relative" in ''|/*|../*|*/../*|*/..|..) echo 'Invalid installed-file inventory' >&2; exit 1 ;; esac
    parent=$(dirname "$payload/$relative")
    while [ "$parent" != "$payload" ]; do
        [ ! -L "$parent" ] || { echo 'An owned parent directory became a symlink; refusing removal' >&2; exit 1; }
        parent=$(dirname "$parent")
    done
    rm -f "$payload/$relative"
done < "$payload/.nerv-files"
find "$payload" -depth -type d -empty -delete
