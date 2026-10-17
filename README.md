# Rustboard

Rust clipboard synchronization with native lazy file presentation and explicitly paired TLS 1.3 peers.
The shared protocol lives in [PROTOCOL.md](PROTOCOL.md), the architecture in
[ARCHITECTURE.md](ARCHITECTURE.md), and security boundaries in [SECURITY.md](SECURITY.md).

- Windows: OLE clipboard/virtual files; see [Windows testing](docs/WINDOWS_TESTING.md).
- macOS: AppKit, APFS snapshots, File Provider and signed local IPC; see [macOS integration](docs/MACOS.md).
- Linux: native backend not yet implemented.

Build the shared workspace with Rust stable: `cargo test --workspace --locked`.
Run independent compatibility vectors with `python3 scripts/check-vectors.py`.
Native build and acceptance steps are platform-specific.

This is development software. Consult [CONTINUE.md](CONTINUE.md), [TEST_RESULTS.md](TEST_RESULTS.md),
and [macOS results](docs/MACOS_RESULTS.md) for PASS/FAIL/BLOCKED/NOT_TESTED evidence.
A successful build or loopback test does not prove native paste, cross-machine interoperability,
production security, signing distribution, or clean-machine installation.
