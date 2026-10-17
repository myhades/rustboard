#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../.."
out="$PWD/target/macos"
# Do not deploy unsigned builds or silently relax Gatekeeper/quarantine.
codesign --verify --deep --strict "$out/Rustboard.app"
team=$(codesign -dv "$out/bin/rustboardd" 2>&1 | sed -n 's/^TeamIdentifier=//p')
[[ "$team" =~ ^[A-Z0-9]{10}$ ]] || { echo 'Run sign.sh first' >&2; exit 1; }
base="$HOME/Library/Application Support/Rustboard"
app="$HOME/Applications/Rustboard.app"
mkdir -p "$base/bin" "$HOME/Applications" "$HOME/Library/LaunchAgents"
chmod 700 "$base" "$base/bin"
# A replacement is limited to the Rustboard installation, never another provider.
if [ -e "$app" ]; then
  id=$(/usr/libexec/PlistBuddy -c 'Print CFBundleIdentifier' "$app/Contents/Info.plist")
  [ "$id" = org.rustboard.host ] || { echo 'Unexpected existing app' >&2; exit 1; }
fi
launchctl bootout "gui/$(id -u)/org.rustboard.agent" 2>/dev/null || true
/usr/bin/ditto "$out/Rustboard.app" "$app"
cp "$out/bin/rustboardd" "$out/bin/rustboard" "$base/bin/"
/usr/bin/pluginkit -a "$app/Contents/PlugIns/RustboardProvider.appex"
"$app/Contents/MacOS/Rustboard" register
python3 scripts/macos/launchd.py "$base" "$app" "$HOME/Library/LaunchAgents/org.rustboard.agent.plist"
launchctl bootstrap "gui/$(id -u)" "$HOME/Library/LaunchAgents/org.rustboard.agent.plist"
for attempt in {1..50}; do
  if "$base/bin/rustboard" doctor 2>/dev/null; then exit 0; fi
  sleep 0.2
done
echo 'Agent failed to start; inspect the installed agent logs' >&2
exit 1
