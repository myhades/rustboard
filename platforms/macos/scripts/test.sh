#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../../.."
mkdir -p target/macos/tests
native=platforms/macos/native
xcrun swiftc -swift-version 5 -O \
  "$native/Security.swift" "$native/Wire.swift" "$native/Provider.swift" \
  "$native/Tests/Tests.swift" "$native/Tests/ProtocolVectors.swift" \
  "$native/Tests/BinaryFraming.swift" "$native/Tests/ProviderMetadata.swift" \
  -o target/macos/tests/native-tests -framework FileProvider
./target/macos/tests/native-tests tests/protocol/vectors.json
