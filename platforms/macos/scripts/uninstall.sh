#!/bin/bash
set -euo pipefail
launchctl bootout "gui/$(id -u)/org.rustboard.agent" 2>/dev/null || true
app="$HOME/Applications/Rustboard.app"
if [ -d "$app" ]; then
  id=$(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "$app/Contents/Info.plist")
  [ "$id" = org.rustboard.host ] || exit 1
  if [ -n "$("$app/Contents/MacOS/Rustboard" status)" ]; then "$app/Contents/MacOS/Rustboard" remove; fi
  pluginkit -r "$app/Contents/PlugIns/RustboardProvider.appex"
  rm -rf "$app"
fi
rm -f "$HOME/Library/LaunchAgents/org.rustboard.agent.plist"
rm -f "$HOME/Library/Application Support/Rustboard/bin/rustboard" "$HOME/Library/Application Support/Rustboard/bin/rustboardd"
python3 "$(dirname "$0")/path.py" remove
echo 'Removed Rustboard app and login agent. Identity, pairing, and diagnostic data are retained.'
