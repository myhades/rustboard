#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../.."
source scripts/macos/env.sh
cargo build --workspace --release --locked
out="$PWD/target/macos"
app="$out/Rustboard.app"
provider="$app/Contents/PlugIns/RustboardProvider.appex"
mkdir -p "$app/Contents/MacOS" "$provider/Contents/MacOS" "$out/bin"
xcrun swiftc -swift-version 5 -O native/macos/Security.swift native/macos/Wire.swift native/macos/main.swift -o "$app/Contents/MacOS/Rustboard" -framework AppKit -framework FileProvider
xcrun swiftc -swift-version 5 -O -emit-executable -parse-as-library -module-name RustboardProvider native/macos/Security.swift native/macos/Wire.swift native/macos/Provider.swift -o "$provider/Contents/MacOS/RustboardProvider" -framework FileProvider -Xlinker -e -Xlinker _NSExtensionMain
cp target/release/rustboardd target/release/rustboard "$out/bin/"
python3 scripts/macos/bundle.py "$out"
printf '%s\n' "Built $app; run scripts/macos/sign.sh with SIGN_IDENTITY before running."
