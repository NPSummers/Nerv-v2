#!/bin/bash
set -euo pipefail
if [[ $# -lt 1 || $# -gt 2 ]]; then
    echo 'Usage: install.sh ARCHIVE.zip [DESTINATION]' >&2
    exit 1
fi
archive=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
destination=${2:-"$HOME/.local/share/nerv"}
if [[ -f "$archive.sha256" ]]; then
    expected=$(awk 'NR == 1 { print $1 }' "$archive.sha256")
else
    expected=$(awk -v name="$(basename "$archive")" '$2 == name { print $1 }' "$(dirname "$archive")/SHA256SUMS")
fi
[[ "$expected" =~ ^[0-9a-fA-F]{64}$ ]] || { echo 'No unique valid archive checksum' >&2; exit 1; }
actual=$(shasum -a 256 "$archive" | awk '{ print $1 }')
if [[ "$expected" != "$actual" ]]; then echo 'Compiler archive checksum mismatch' >&2; exit 1; fi
if [[ -e "$destination" ]]; then echo 'Destination exists; choose a new directory' >&2; exit 1; fi
mkdir -p "$(dirname "$destination")"
staging=$(mktemp -d "$(dirname "$destination")/.nerv-install.XXXXXXXX")
trap 'rm -rf -- "$staging"' EXIT
while IFS= read -r entry; do
    case "$entry" in /*|../*|*/../*|*\\*) echo 'Invalid archive path' >&2; exit 1 ;; esac
done < <(unzip -Z -1 "$archive")
unzip -q "$archive" -d "$staging"
shopt -s nullglob
directories=("$staging"/*)
if [[ ${#directories[@]} -ne 1 || ! -f "${directories[0]}/nerv" ]]; then echo 'Invalid Nerv compiler archive' >&2; exit 1; fi
chmod +x "${directories[0]}/nerv"
mv "${directories[0]}" "$destination"
echo "Installed Nerv in $destination"
echo 'Add this directory to PATH, then run nerv --version.'
