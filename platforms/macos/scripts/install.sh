#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../../.."
out="$PWD/target/macos"
if [ -d "$PWD/Rustboard.app" ] && [ -d "$PWD/bin" ]; then out="$PWD"; fi
listen=""
manage_path=true
while [ "$#" -gt 0 ]; do
  case "$1" in
    --listen) [ "$#" -ge 2 ] || exit 64; listen="$2"; shift 2 ;;
    --no-path) manage_path=false; shift ;;
    *) echo 'Usage: install.sh [--listen IP:port] [--no-path]' >&2; exit 64 ;;
  esac
done
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
v=json.loads(path.read_text()) if path.exists() else dict(schema=2,listen=None,mode='all',peers=[],identity=None)
if sys.argv[2]:
    host,port=sys.argv[2].rsplit(':',1); ipaddress.ip_address(host.strip('[]'))
    if not 0<int(port)<65536:raise SystemExit('Invalid port')
    v['listen']=sys.argv[2]
# Fresh installs may start unpaired with networking off.
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
enabled=true
if launchctl print-disabled "gui/$(id -u)" | grep -Eq '"org.rustboard.agent" => (true|disabled)'; then enabled=false; fi
launchctl bootout "gui/$(id -u)/org.rustboard.agent" 2>/dev/null || true
/usr/bin/ditto "$out/Rustboard.app" "$app"
# Replacing an inode avoids stale executable signature pages after an upgrade.
for name in rustboardd rustboard; do
  temporary=$(mktemp "$base/bin/.$name.XXXXXX")
  if ! cp -p "$out/bin/$name" "$temporary"; then rm -f "$temporary"; exit 1; fi
  mv -f "$temporary" "$base/bin/$name"
done
/usr/bin/pluginkit -a "$app/Contents/PlugIns/RustboardProvider.appex"
if ! "$app/Contents/MacOS/Rustboard" register; then
  echo "Registration failed. Check System Settings > Privacy & Security for an Open Anyway prompt, then retry." >&2
  exit 1
fi
if $manage_path; then python3 platforms/macos/scripts/path.py add; fi
python3 platforms/macos/scripts/launchd.py "$base" "$app" "$HOME/Library/LaunchAgents/org.rustboard.agent.plist"
if ! $enabled; then echo 'Updated; service remains disabled. Use rustboard start to enable.'; exit 0; fi
launchctl enable "gui/$(id -u)/org.rustboard.agent"
launchctl bootstrap "gui/$(id -u)" "$HOME/Library/LaunchAgents/org.rustboard.agent.plist"
for attempt in {1..150}; do
  if "$base/bin/rustboard" status --json 2>/dev/null; then
    echo "CLI ready. Open a new terminal after direct installation. Configure listen/peers with rustboard config; apply with rustboard reload."
    echo "Permissions: see docs/INSTALLATION.md for manual System Settings instructions."
    exit 0
  fi
  sleep 0.2
done
echo 'Agent failed to start; inspect the installed agent logs' >&2
exit 1
