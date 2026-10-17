#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../.."
out="$PWD/target/macos"
if [ -d "$PWD/Rustboard.app" ] && [ -d "$PWD/bin" ]; then out="$PWD"; fi
listen=""
if [ "$#" -gt 0 ]; then
  [ "$#" -eq 2 ] && [ "$1" = --listen ] || { echo 'Usage: install.sh --listen LAN-IP:port' >&2; exit 1; }
  listen="$2"
fi
export RUSTBOARD_PROFILE=default
# Do not deploy unsigned builds or silently relax Gatekeeper/quarantine.
codesign --verify --deep --strict "$out/Rustboard.app"
codesign --verify --strict "$out/bin/rustboardd"
codesign --verify --strict "$out/bin/rustboard"
team=$(codesign -dv "$out/bin/rustboardd" 2>&1 | sed -n 's/^TeamIdentifier=//p')
[[ "$team" =~ ^[A-Z0-9]{10}$ ]] || { echo 'Run sign.sh first' >&2; exit 1; }
config_dir="$HOME/Library/Group Containers/$team.org.rustboard.shared/default"
python3 - "$config_dir" "$listen" <<'PYCONFIG'
import sys, json, os, ipaddress
from pathlib import Path
os.umask(0o077)
directory=Path(sys.argv[1]); directory.mkdir(parents=True,exist_ok=True)
path=directory/'config.json'
v=json.loads(path.read_text()) if path.exists() else dict(listen=None,mode='all')
if sys.argv[2]:
    host,port=sys.argv[2].rsplit(':',1); ipaddress.ip_address(host.strip('[]'))
    if not 0<int(port)<65536:raise SystemExit('Invalid port')
    v['listen']=sys.argv[2]
if not v.get('listen'):raise SystemExit('First install requires --listen LAN-IP:port')
temporary=directory/'config.new';temporary.write_text(json.dumps(v,indent=2));temporary.replace(path);path.chmod(0o600)
PYCONFIG
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
launchctl enable "gui/$(id -u)/org.rustboard.agent"
launchctl bootstrap "gui/$(id -u)" "$HOME/Library/LaunchAgents/org.rustboard.agent.plist"
for attempt in {1..50}; do
  if "$base/bin/rustboard" status --json 2>/dev/null; then exit 0; fi
  sleep 0.2
done
echo 'Agent failed to start; inspect the installed agent logs' >&2
exit 1
