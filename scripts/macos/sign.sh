#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../.."
: "${SIGN_IDENTITY:?Set SIGN_IDENTITY to an installed Apple signing identity hash}"
out="$PWD/target/macos"
codesign --force --sign "$SIGN_IDENTITY" --identifier org.rustboard.agent "$out/bin/rustboardd"
team=$(codesign -dv "$out/bin/rustboardd" 2>&1 | sed -n 's/^TeamIdentifier=//p')
[[ "$team" =~ ^[A-Z0-9]{10}$ ]] || { echo 'A real Apple signing team is required' >&2; exit 1; }
python3 - "$out" "$team" <<'PY'
import sys,plistlib
from pathlib import Path
out=Path(sys.argv[1]); group=sys.argv[2]+'.org.rustboard.shared'
for name in ['Host','Provider','Agent']:
    value=plistlib.loads((Path('native/macos')/(name+'.entitlements.in')).read_bytes())
    value['com.apple.security.application-groups']=[group]
    (out/(name+'.entitlements')).write_bytes(plistlib.dumps(value))
p=out/'Rustboard.app/Contents/PlugIns/RustboardProvider.appex/Contents/Info.plist'
v=plistlib.loads(p.read_bytes());v['NSExtension']['NSExtensionFileProviderDocumentGroup']=group;p.write_bytes(plistlib.dumps(v))
PY
codesign --force --options runtime --sign "$SIGN_IDENTITY" --identifier org.rustboard.agent --entitlements "$out/Agent.entitlements" "$out/bin/rustboardd"
codesign --force --options runtime --sign "$SIGN_IDENTITY" --identifier org.rustboard.cli --entitlements "$out/Agent.entitlements" "$out/bin/rustboard"
codesign --force --options runtime --sign "$SIGN_IDENTITY" --entitlements "$out/Provider.entitlements" "$out/Rustboard.app/Contents/PlugIns/RustboardProvider.appex"
codesign --force --options runtime --sign "$SIGN_IDENTITY" --entitlements "$out/Host.entitlements" "$out/Rustboard.app"
codesign --verify --deep --strict "$out/Rustboard.app"
printf 'Signed for team %s. This does not imply notarization.\n' "$team"
