#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../../.."
source platforms/macos/scripts/env.sh
export MACOSX_DEPLOYMENT_TARGET=13.0
cargo build --release --locked
out="$PWD/target/macos"
app="$out/Rustboard.app"
provider="$app/Contents/PlugIns/RustboardProvider.appex"
mkdir -p "$app/Contents/MacOS" "$provider/Contents/MacOS" "$out/bin"
xcrun swiftc -target "$(uname -m)-apple-macos13.0" -swift-version 5 -O platforms/macos/native/Security.swift platforms/macos/native/Wire.swift platforms/macos/native/main.swift -o "$app/Contents/MacOS/Rustboard" -framework AppKit -framework FileProvider
xcrun swiftc -target "$(uname -m)-apple-macos13.0" -swift-version 5 -O -emit-executable -parse-as-library -module-name RustboardProvider platforms/macos/native/Security.swift platforms/macos/native/Wire.swift platforms/macos/native/Provider.swift -o "$provider/Contents/MacOS/RustboardProvider" -framework FileProvider -Xlinker -e -Xlinker _NSExtensionMain
cp target/release/rustboardd target/release/rustboard "$out/bin/"
python3 platforms/macos/scripts/bundle.py "$out"
printf '%s\n' "Built $app; run platforms/macos/scripts/sign.sh with SIGN_IDENTITY before running."
