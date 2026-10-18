#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../../.."
source platforms/macos/scripts/env.sh
mkdir -p target/macos/tests test-output/checks/macos
cargo test --locked 2>&1 | tee test-output/checks/macos/rust-tests.txt
python3 scripts/check-vectors.py | tee test-output/checks/macos/python-vectors.txt
xcrun swiftc -swift-version 5 -O platforms/macos/native/Security.swift platforms/macos/native/Wire.swift platforms/macos/native/Tests/ProtocolVectors.swift -o target/macos/tests/protocol-vectors
./target/macos/tests/protocol-vectors tests/protocol/vectors.json | tee test-output/checks/macos/swift-vectors.txt
xcrun swiftc -swift-version 5 -O platforms/macos/native/Security.swift platforms/macos/native/Wire.swift platforms/macos/native/Tests/BinaryFraming.swift -o target/macos/tests/binary-framing
./target/macos/tests/binary-framing | tee test-output/checks/macos/binary-framing.txt
xcrun swiftc -swift-version 5 -O platforms/macos/native/Security.swift platforms/macos/native/Wire.swift platforms/macos/native/Provider.swift platforms/macos/native/Tests/ProviderMetadata.swift -o target/macos/tests/provider-metadata -framework FileProvider
./target/macos/tests/provider-metadata | tee test-output/checks/macos/provider-metadata.txt
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
