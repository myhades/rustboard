# Architecture

Rustboard synchronizes supported clipboard contents between explicitly paired devices. It has no moving/recycling policy, sensitive-content switch, background file cache database or plugin framework. Files always copy. Native image bytes are exposed as ordinary image files; rich text uses the application's plain-text representation.

## Repository

```text
crates/
  core/                 portable protocol, Engine, ReadAt, TLS and peer enrollment
  app/                  configuration, CLI, Windows/macOS adapters and two bin targets
platforms/
  windows/scripts/      install, package and platform acceptance helpers
  macos/
    native/             AppKit bridge, minimal host and File Provider extension
    scripts/            build, signing, install, package and acceptance helpers
    packaging/          Homebrew template
scripts/                shared verification scripts
tests/
  protocol/             independent protocol vectors
  support/              test-only pattern sources and fixture identity storage
tools/rust/             standalone diagnostic/example executables
docs/                  current documentation, historical reports and evidence
```

Ubuntu will use `crates/app/src/ubuntu/` plus `platforms/ubuntu/` when implemented. No empty placeholder backend pretends to support Linux clipboard operations today. The portable core is tested on Ubuntu CI.

## Why two crates

`rustboard-core` has no native API dependencies. It owns validation, arbitration, bounded retained file batches, authenticated TLS and a portable random-access source contract. `rustboard-app` depends on it and owns operating-system permissions, configuration, Keychain/DPAPI, service lifecycle, IPC endpoints and clipboard presentation.

One crate is technically possible, but would rely solely on convention to preserve this boundary. Two crates provide compiler-enforced dependency direction and portable tests. Two additional crates for tiny entry points added no such boundary, so both binaries now live in app/src/bin. Keep two executables: a console CLI and a silent Windows GUI-subsystem Agent serve different native purposes.

## Runtime

Native event -> capture -> Engine.local -> metadata-only authenticated Offer -> peer's native publication. File reads start only when a consumer asks. Windows uses IDataObject/IStream backed by ReadAt. macOS passes the Offer directly to the host, which publishes dataless URLs; the extension reads through authenticated local Agent IPC. The Mac adapter does not create unused remote stream wrappers.

Platform publication uses direct methods; no generic clipboard/backend trait is needed because no shared caller consumes one. ReadAt remains shared because local handles, snapshots and remote streams actually implement the same bounded read contract.

The Agent uses bounded blocking workers, with a native main-thread event loop (Windows OLE STA; macOS AppKit). Retained file batches survive a new clipboard Offer. Cancellation, expiry and authorization still bound their lifetime. See PROTOCOL.md. macOS host, extension and Agent remain separate because File Provider sandbox/process integration requires them.

The CLI changes saved config/peers; reload restarts the Agent. It does not hot-swap trust during a transfer. Production identities use Keychain/DPAPI. Synthetic sources, plain fixture identity files, demos and metrics-only harnesses live outside production modules and are not compiled into release binaries.
