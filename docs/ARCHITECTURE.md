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

Native event -> capture -> Engine.local -> metadata-only authenticated Offer -> peer's native publication. File reads are lazy by default; optional Mac eager download uses the same path for batches below 50 MB. Windows uses IDataObject/IStream backed by ReadAt. macOS passes the Offer directly to the host, which publishes dataless URLs; the extension reads through authenticated local Agent IPC. The Mac adapter does not create unused remote stream wrappers.

Platform publication uses direct methods; no generic clipboard/backend trait is needed because no shared caller consumes one. ReadAt remains shared because local handles, snapshots and remote streams actually implement the same bounded read contract.

The Agent uses bounded blocking workers, with a native main-thread event loop (Windows OLE STA; macOS AppKit). Only current batches and explicitly owned transfers survive replacement. A TLS ownership connection bridges independent read-channel reconnects; native TCP keepalive detects lost peers without an application heartbeat. Windows shares ownership across a multi-file data object; File Provider exposes individual fetch lifetimes. See PROTOCOL.md. macOS host, extension and Agent remain separate because File Provider sandbox/process integration requires them.

The CLI changes saved config/peers; reload restarts the Agent. It does not hot-swap trust during a transfer. Production identities use Keychain/DPAPI. Synthetic sources, plain fixture identity files, demos and metrics-only harnesses live outside production modules and are not compiled into release binaries.

## Before release automation and Linux

The single package keeps portable file I/O in `core/source.rs`; Windows handle
locking and path/change-time checks live in `platform/windows/local_files.rs`.
`core/transport.rs` owns the Node and serving loop. Its concrete child modules
separate TLS identity/pinning (`tls`), binary framing (`framing`), native IPC
(`ipc`), and remote reads/batch ownership (`transfer`); `pairing` stays separate.
This is a file-level separation with the existing ReadAt contract, not a new
backend framework or protocol layer.

CI currently runs Cargo checks on three systems. Add native Swift tests and full
host/provider builds, fixed toolchain and minimum-version checks, and a tag-driven
archive/checksum/package-manifest pipeline. Publish the tested artifacts. Mac
public distribution requires Developer ID and notarization; development signing
does not replace those steps. Exercise package-manager install/update/removal in
a clean account, including PATH ownership and identity preservation.

Linux currently tests portable code, not a native clipboard backend. Choose the
first desktop/compositor explicitly and prototype clipboard access and lazy file
URLs there. The XDG Clipboard portal is tied to a RemoteDesktop session, so silent
background clipboard access cannot be assumed across all Wayland desktops. Then
add platform-local credential storage, authenticated IPC, user service and
packaging using the existing protocol and transfer lifetime.

References: [XDG Clipboard portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Clipboard.html),
[Apple Developer ID](https://developer.apple.com/developer-id/).
