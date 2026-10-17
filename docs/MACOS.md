# macOS integration

The macOS implementation is in `crates/rustboard-platform/src/macos` and `native/macos`.
It uses the existing Rust Node and protocol v1, not the former lazy_paste POC transport.
Current evidence and release gates are in [MACOS_RESULTS.md](MACOS_RESULTS.md) and [CONTINUE.md](../CONTINUE.md).

## Build and sign

macOS 13+, Xcode command-line tools, Rust stable. The tested machine is arm64, Xcode 27.

```sh
scripts/macos/test.sh
scripts/macos/build.sh
SIGN_IDENTITY='<installed Apple signing identity hash>' scripts/macos/sign.sh
```

`env.sh` prefers cargo on PATH or ~/.cargo/bin; on this development machine it can reuse the
POC's local Rust toolchain without changing global settings. No POC source, identity, token,
or protocol is used at runtime. `Cargo.lock` changed only to record the target-specific libc dependency.

Signing uses the actual signature's TeamIdentifier, never the name printed in the certificate CN.
App group is `<TeamIdentifier>.org.rustboard.shared`. Apple documents this macOS naming scheme
in [Accessing app group containers](https://developer.apple.com/documentation/xcode/accessing-app-group-containers).
Host and extension are sandboxed; the Rust agent is a user-session process. All processes use the
same signing team and explicit Rustboard identifiers. Build/sign artifacts and entitlements are in
`target/macos`, not source-controlled credentials. Development signing is not Developer ID notarization.

On this machine headless codesign returns NO_USER_INTERACTION; the same script succeeded in a
normal Terminal session. Run sign.sh there if necessary. Do not change keychain ACLs, security policy,
or remove quarantine as a workaround.

## Native architecture and security

AppKit owns the main thread, polls pasteboard changeCount, captures text/file URL metadata, and
suppresses its publication sequence. An external clipboard change during asynchronous File Provider
URL preparation aborts publication. The sandboxed host publishes the original security-scoped
File Provider URLs as explicit pasteboard items and retains their scopes while pboard consumes
the grants. The Agent verifies the sequence and complete file count from its own process after
host exit before adopting ownership; incomplete publication is cleared and retried. This avoids
both launchd path-string authorization loss and asynchronous partial pasteboard publication.
There is no keyboard listener, simulated paste, or GUI frontend.

Local file offers hold APFS COW clones in a private directory. Unsupported filesystems, symlink files,
directories and dataless source files are rejected; there is no full-byte-copy fallback. A clone freezes
one filesystem version without copy-time hashing. Snapshots are removed when sources release and
on the next singleton Agent startup. Source read requests are bounded to 64 KiB.

The Unix socket is in the app group profile directory (0700), mode0600; a no-follow flock file prevents
duplicate Agents. Both sides check peer UID and the kernel LOCAL_PEERTOKEN, then evaluate Apple
signature anchor, same team and allowed bundle identifier. Sandboxed clients use live dynamic code
validation of the non-bundled Agent, avoiding opening an executable outside their container. This is
based on the [Security guest attributes API](https://developer.apple.com/documentation/security/guest-attribute-dictionary-keys).
A fetch reuses an authenticated socket with sequential v1 frames; the server admits at most eight clients with bounded I/O timeouts. The adapter passes get_offer/read/cancel directly to Node.local_ipc. Control is native. There is no local TCP endpoint.

Extension identifiers include source device, offer, file and SHA256(version); the original version
is transmitted in reads. Known-size flat file offers are supported; nested paths/unknown sizes fail.
The live Offer is the authority; a mode0600 metadata-only publication index supports eviction of
previously presented items. A bounded retirement ledger supports explicit deletion notifications; initial sync anchors are 32-byte digests and change anchors are 9-byte generation tokens (the OS limit is 500 bytes). A locked metadata ledger reclaims retired IDs only when the OS returns an acknowledged generation; it fails closed beyond 4096 unacknowledged IDs. It is not an authorization or content cache. Restart does not restore Offers.

Finder export items advertise writable presentation to avoid the immutable `uchg` flag breaking repeated copies. Provider create/modify/delete remain unsupported; this is not remote file editing or a bidirectional filesystem.

Enumeration creates metadata with downloadLazily. fetchContents checks the tuple, streams 64KiB
reads into an exclusive0600 random file, validates request ID/offset/count/EOF and rechecks current
Offer before delivery. Cancellation, errors and stale Offers remove that temporary file. At most two
hydrations run concurrently. Shared Node.local_ipc retains at most two TLS connections keyed by the full tuple, reauthorizes each read, and releases them on EOF/cancel; it does not cache file bytes. Per-chunk autorelease pools bound Foundation temporary allocations.
No successful callback returns an incomplete file. System File Provider may hydrate in the background;
record callback timing instead of assuming that every read comes from Cmd+V.

The OS owns materialized File Provider caches. Replaced/expired Offers request eviction; this is not
an independently proven guarantee that every OS-cached/open file becomes unreadable immediately.
That revocation/cache behavior remains a native acceptance gate. Same-user malware is outside the
shared repository's security boundary; no claim of protection against a compromised signing key.

## Install / launchd / Homebrew

After signing and native validation:

```sh
scripts/macos/install.sh
scripts/macos/uninstall.sh
python3 scripts/macos/package.py
```

Install deploys the app under ~/Applications and binaries under ~/Library/Application Support/Rustboard,
registers the hidden domain, then installs a current-user LaunchAgent. This avoids running a background
executable from Documents. No root service or additional persistent helper is used. Network listening
is opt-in; the generated default job does not listen. Use an explicitly configured --listen IP:port in
ProgramArguments when needed, then pair fingerprints on BOTH devices over a trusted channel.
No firewall settings are changed. `rustboard stop` is transient under KeepAlive; bootout stops the job.

Uninstall removes only the named Rustboard installation/domain and keeps identity/pairing/log data.
`package.py` generates a local tar.gz and a SHA256-pinned development Homebrew formula from
`packaging/macos/rustboard.rb.in`. It is not a published tap, notarized release, or proof of clean-Mac
installation. The formula deploys files without auto-granting system permissions. Do not distribute
Apple Development artifacts as a production Developer ID release.

## Reproducible acceptance

First run shared Rust and Python vectors, then Swift tests. Optional full-size library stress test:

```sh
xcrun swiftc -swift-version 5 -O native/macos/Security.swift native/macos/Wire.swift native/macos/Tests/LargeHydration.swift -o target/macos-large-test
/usr/bin/time -l target/macos-large-test tests/protocol/vectors.json
```

This injects a transport into the actual hydration loop and verifies all shared hashes. It is NOT Finder,
File Provider callback, sandbox, network throughput or Windows interoperability evidence.

After build/sign and building `cargo build -p rustboard-platform --example macos_acceptance_peer --release`:

```sh
python3 scripts/macos/acceptance.py prepare --sizes 4096
python3 scripts/macos/acceptance.py status
# Record dataless flags, blocks=0, source reads=0, and provider logs BEFORE paste.
# In Finder navigate to an empty test destination and press real Cmd+V.
python3 scripts/macos/verify-files.py /absolute/synthetic/destination --sizes 4096
python3 scripts/macos/acceptance.py stop
```

Use default sizes for the 4KiB/64MiB/1GiB multi-file run. Collect `log stream --predicate
'subsystem == "org.rustboard"'` for fetch_start/fetch_complete/fetch_failed; inspect only Rustboard
CloudStorage items with stat, not other providers. Collect ps/RSS and source reads.json before and
after. Repeat paste with and without eviction and explicitly identify cache hits. Test pause during
hydration, peer disconnect, newer Offer, expiration, Agent crash/restart and denied unsigned clients.
The profile is isolated as `acceptance`, and both TLS endpoints bind loopback. Test artifacts are
under ignored test-output/macos/acceptance. Do not label this cross-machine testing.

For Windows, record both actual device fingerprints, both directions of Unicode text and multi-file
Finder/Explorer paste, independent target hashes, cancellation, permissions and restart. The Windows
maintainer's earlier Explorer results are not macOS results. A second physical endpoint is still required.
