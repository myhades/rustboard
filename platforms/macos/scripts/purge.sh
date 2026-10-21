#!/bin/bash
set -euo pipefail
# Homebrew runs the uninstall hook before this zap hook.
while true; do
  if message=$(security delete-generic-password -s org.rustboard.device-identity 2>&1); then
    continue
  else
    result=$?
    [ "$result" -eq 44 ] && break
    printf '%s\n' "$message" >&2
    exit "$result"
  fi
done
rm -rf "$HOME/Library/Application Support/Rustboard"
for group in "$HOME/Library/Group Containers/"*.org.rustboard.shared; do
  [ -e "$group" ] || continue
  rm -rf "$group"
done
for identifier in org.rustboard.host org.rustboard.provider; do
  rm -rf "$HOME/Library/Containers/$identifier" "$HOME/Library/Caches/$identifier" \
    "$HOME/Library/Saved Application State/$identifier.savedState"
  rm -f "$HOME/Library/Preferences/$identifier.plist"
done
