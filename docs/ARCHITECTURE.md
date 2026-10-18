# Architecture

Rustboard synchronizes supported clipboard contents between explicitly paired devices. It has no moving/recycling policy, sensitive-content switch, background file cache database or plugin framework. Files always copy. Native image bytes are exposed as ordinary image files; rich text uses the application's plain-text representation.

## Repository

```text
Cargo.toml              one package, library plus two binary targets
build.rs                macOS bridge build
src/
  lib.rs
  core/                 portable protocol, Engine, ReadAt and TLS
  app/                  CLI, configuration and shared application logic
  platform/windows/     Win32/COM integration
  platform/macos/       AppKit/IPC integration
  bin/rustboard.rs       console CLI
  bin/rustboardd.rs      background Agent
platforms/              native assets, scripts and packaging by operating system
tools/rust/             standalone developer tools (Cargo examples)
tests/                  protocol vectors, integration tests and test-only support
docs/                   current docs, archived handoffs and evidence
```

Ubuntu can add `src/platform/ubuntu/` and `platforms/ubuntu/` when implemented; there is no placeholder clipboard backend. Ubuntu CI checks the portable code and unsupported-platform entry points.

## Package and executable boundaries

One package is sufficient for this application. Core and platform responsibilities are ordinary modules; there are no internal path dependencies, facade crates or backend abstraction layers. Native dependencies remain selected with Cargo target conditions and Rust cfg attributes.

The two executables retain their existing roles: a console CLI and a silent Windows GUI-subsystem Agent. They share the same library and configuration implementation. macOS still requires its native File Provider extension and minimal host app.

## Runtime

Native event -> capture -> Engine.local -> metadata-only authenticated Offer -> peer's native publication. File reads start only when a consumer asks. Windows uses IDataObject/IStream backed by ReadAt. macOS passes the Offer directly to the host, which publishes dataless URLs; the extension reads through authenticated local Agent IPC. The Mac adapter does not create unused remote stream wrappers.

Platform publication uses direct methods; no generic clipboard/backend trait is needed because no shared caller consumes one. ReadAt remains shared because local handles, snapshots and remote streams actually implement the same bounded read contract.

The Agent uses bounded blocking workers, with a native main-thread event loop (Windows OLE STA; macOS AppKit). Retained file batches survive a new clipboard Offer. Cancellation, expiry and authorization still bound their lifetime. See PROTOCOL.md. macOS host, extension and Agent remain separate because File Provider sandbox/process integration requires them.

The CLI changes saved config/peers; reload restarts the Agent. It does not hot-swap trust during a transfer. Production identities use Keychain/DPAPI. Synthetic sources, plain fixture identity files, demos and metrics-only harnesses live outside production modules and are not compiled into release binaries.
