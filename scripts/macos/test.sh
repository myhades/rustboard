#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../.."
source scripts/macos/env.sh
mkdir -p target/macos/tests docs/test-results/macos
cargo test --workspace --locked 2>&1 | tee docs/test-results/macos/rust-tests.txt
python3 scripts/check-vectors.py | tee docs/test-results/macos/python-vectors.txt
xcrun swiftc -swift-version 5 -O native/macos/Security.swift native/macos/Wire.swift native/macos/Tests/Compatibility.swift -o target/macos/tests/compatibility
./target/macos/tests/compatibility tests/protocol/vectors.json | tee docs/test-results/macos/swift-vectors.txt
xcrun swiftc -swift-version 5 -O native/macos/Security.swift native/macos/Wire.swift native/macos/Tests/DecodeCompatibility.swift -o target/macos/tests/decode-compatibility
./target/macos/tests/decode-compatibility | tee docs/test-results/macos/decode-compatibility.txt
xcrun swiftc -swift-version 5 -O native/macos/Security.swift native/macos/Wire.swift native/macos/Provider.swift native/macos/Tests/ProviderMetadata.swift -o target/macos/tests/provider-metadata -framework FileProvider
./target/macos/tests/provider-metadata | tee docs/test-results/macos/provider-metadata.txt
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
