#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../../.."
out="$PWD/target/macos"
if [ -d "$PWD/Rustboard.app" ] && [ -d "$PWD/bin" ]; then out="$PWD"; fi
manage_path=true
while [ "$#" -gt 0 ]; do
  case "$1" in
    --no-path) manage_path=false; shift ;;
    *) echo 'Usage: install.sh [--no-path]' >&2; exit 64 ;;
  esac
done
base="$HOME/Library/Application Support/Rustboard"
app="$HOME/Applications/Rustboard.app"
mkdir -p "$base/bin" "$HOME/Applications"
chmod 700 "$base" "$base/bin"
# A replacement is limited to the Rustboard installation, never another provider.
if [ -e "$app" ]; then
  id=$(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "$app/Contents/Info.plist")
  [ "$id" = org.rustboard.host ] || { echo 'Unexpected existing app' >&2; exit 1; }
fi
/usr/bin/ditto "$out/Rustboard.app" "$app"
# Replacing an inode avoids stale executable signature pages after an upgrade.
for name in rustboardd rustboard; do
  temporary=$(mktemp "$base/bin/.$name.XXXXXX")
  if ! cp -p "$out/bin/$name" "$temporary"; then rm -f "$temporary"; exit 1; fi
  mv -f "$temporary" "$base/bin/$name"
done
if $manage_path; then python3 platforms/macos/scripts/path.py add; fi
printf 'Installed in %s.\n' "$base"
