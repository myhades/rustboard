# Architecture

Rustboard synchronizes supported clipboard contents between explicitly paired devices. Files always copy. Native image bytes are exposed as ordinary image files; rich text uses the application's plain-text representation.

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

Ubuntu CI checks the portable code and unsupported-platform entry points.

## Package and executable boundaries

Native dependencies remain selected with Cargo target conditions and Rust cfg attributes.

The two executables retain their existing roles: a console CLI and a silent Windows GUI-subsystem Agent. They share the same library and configuration implementation. macOS still requires its native File Provider extension and minimal host app.

## Runtime

Native event -> capture -> Engine.local -> metadata-only authenticated Offer -> peer's native publication. File reads are lazy by default; optional Mac eager download uses the same path for batches below 50 MB. Windows uses IDataObject/IStream backed by ReadAt. macOS passes the Offer directly to the host, which publishes dataless URLs; the extension reads through authenticated local Agent IPC.

The Agent uses bounded blocking workers, with a native main-thread event loop (Windows OLE STA; macOS AppKit). Only current batches and explicitly owned transfers survive replacement. A TLS ownership connection bridges independent read-channel reconnects; native TCP keepalive detects lost peers without an application heartbeat. Windows shares ownership across a multi-file data object; File Provider exposes individual fetch lifetimes. See PROTOCOL.md. macOS host, extension and Agent remain separate because File Provider sandbox/process integration requires them.

The CLI changes saved config/peers; restart restarts the Agent. It does not hot-swap trust during a transfer. Production identities use Keychain/DPAPI. Synthetic sources, plain fixture identity files, demos and metrics-only harnesses live outside production modules and are not compiled into release binaries.
