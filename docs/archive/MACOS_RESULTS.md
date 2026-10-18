# macOS evidence — 2026-10-17

This is a development integration with native Finder evidence; physical Windows interoperability remains blocked. No remote push.

**Overall acceptance: FAIL performance / BLOCKED physical Windows interop.** Functional hash
passes below do not establish usable performance: final 1GiB loopback hydration took 383.093s
(~2.67MiB/s). The bottleneck is not yet profiled. Same-volume APFS clone latency was not measured
as a separate end-to-end benchmark. Performance work is handed to the main maintainer.
Raw compact evidence is under `docs/test-results/macos`. The pre-existing POC and Windows
maintainer results are background only and are not counted as Rustboard macOS passes.

| Check | Status | Evidence / boundary |
|---|---|---|
| Repository and POC access, handoff review | PASS | Main repo cloned; POC inspected locally. Original main commit lacked README; a common README was added. |
| Shared Rust Core baseline | PASS | core-initial.txt: 7 unit + 1 compatibility tests |
| Current workspace Rust tests | PASS | rust-tests.txt: includes APFS mutation/cleanup and native Node IPC validation |
| Python vectors | PASS | python-vectors.txt: original 4KiB/64MiB/1GiB pattern hashes |
| Swift v1 compatibility | PASS | swift-vectors.txt: same vectors, Unicode, UInt64 above 2^53 and near u64::MAX, framing, invalid fields/bytes/paths |
| Swift hydration fault cleanup | PASS | swift-vectors.txt: cancellation, disconnect, mismatched ID/offset/EOF, no progress, version failure, withdrawn Offer; injected transport |
| 4KiB/64MiB/1GiB hydration hashes | PASS | large-hydration.txt; actual hydration loop with injected data, not File Provider/Finder/TLS |
| Large-file library RSS | PASS after fix | large-resources.txt: 8,912,896 bytes maximum RSS. Before-fix evidence retained: autoreleased hash-read buffers accumulated ~1.15GB; fixed local pools. Not native-extension/network resource evidence. |
| AppKit publication/capture | PASS | appkit.txt: Unicode, own sequence suppression, external text/file URL metadata, clear only owned clipboard |
| Release Rust + Swift host/extension build | PASS | build-release.txt |
| fmt / Clippy -D warnings | PASS | check.txt |
| First Apple Development signing | PASS | signing-gui.txt; same TeamIdentifier for Agent/CLI/host/extension; signature verification. Not notarization. |
| Final host/clipboard fixes signing | PASS | User signing session resumed; final request 1b9cb73a817244e2819e925f8a753231 succeeded. No private-key ACL changes; not notarized. |
| Authenticated Rust CLI to Agent IPC | PASS (first signed build) | doctor.txt, ipc-security.txt |
| Same-user unauthorized application rejection | PASS (first signed build) | ipc-security.txt: ordinary Python denied; socket0600 and parent0700 |
| Different-user IPC rejection | NOT_TESTED | No second-account adversarial test |
| Signed app-group access and hidden domain registration | PASS (first signed build) | Runtime created rustboard-v1-default, hidden=true; group container resolved. Does not prove extension hydration. |
| TLS loopback Offer and zero pre-paste body reads | PASS (first signed build) | loopback-pair.txt, source-reads-before-paste.json: 4096-byte source, bytes=0. Both pins explicitly paired in isolated loopback. |
| Sandboxed host-to-Agent authentication / file publication | PASS after fixes | Dynamic audit-token validation works. Darwin inherited O_NONBLOCK fixed; ipc-repeat.txt contains 30 successful signed CLI requests. |
| Native fetchContents callback / real Finder Cmd+V | PASS (4KiB) | finder-small-provider.txt and finder-small-hash.txt; source dataless/zero reads before paste. |
| Native 4KiB/64MiB/1GiB multi-file hashes | PASS final source | release-finder-hash.txt, release-provider.txt; real Finder Cmd+V under restarted launchd Agent. release-before-* show all dataless/0 blocks/zero body reads. Earlier M7 evidence retained. |
| Native resources | PASS sampled | release-full-rss.json: 599.3s sampled, Agent39,878,656 bytes / Provider32,538,624 bytes. Sampling starts just after paste and spans both large transfers; 1GiB fetch383.093s, 64MiB23.942s. These are 1-second sample peaks, not continuous exact maxima. |
| Native Finder cancellation | PASS | finder-cancel-provider.txt: fetch_failed after user-interface Stop; destination empty, source counter stopped at 179437568 bytes for 1GiB. No partial result returned. |
| File Provider anchor size regression | PASS | anchor-crash.txt records >500-byte failure; provider-metadata.txt checks fixed 32-byte initial anchors for 256 items. Change anchors are 9-byte generations. |
| Native repeated paste | PASS final source | release-repeat-hash.txt: all3 hashes; release-after-reads.json equals release-repeat-reads.json, proving OS cache hit. No permission error. Content editing remains unsupported. |
| Native expiry / Offer replacement / TLS peer disconnect | PASS | finder-expiry*, finder-replaced*, finder-disconnect*: actual Finder failure, empty target and pending temp directory. Same-Offer version mutation remains injection-only. |
| Provider crash / pending cleanup / automatic retry | PASS | provider-kill-before.json, provider-restart*.txt: SIGKILL at17MB, orphan removed by new process, 64MiB target hash correct. |
| Old metadata removal after withdrawal | PASS | metadata-after-withdraw.txt: domain empty after text replacement. Open OS-cached handles cannot be revoked retroactively. |
| User launchd start, SIGKILL recovery, real authenticated CLI service | PASS | launchd-current.txt: corrected signed build PID68802→68822; identity stable, doctor before/after; temporary job booted out. |
| launchd + real provider fetch after restart | PASS | release-build.json, release-finder-hash.txt, release-provider.txt: final Agent PID84423 after kickstart; all three hashes passed. Earlier empty/partial pasteboard failures preserved. |
| Login / full machine restart | NOT_TESTED | No persistent login job installed, no reboot. |
| User installation / repeated installation / uninstall | PASS | install-lifecycle.txt: domain name collision fixed, installation and same-version replacement started launchd with stable identity, uninstall removed job/app. |
| Signed package / Homebrew lifecycle | PASS | homebrew.txt: local temporary tap install, formula test, same-version reinstall, uninstall and untap. Actual cross-version upgrade NOT_TESTED; no release published. |
| Windows physical device bidirectional interop | BLOCKED | User states Windows side is not running; no IP/fingerprint supplied. |
| AppKit text in both loopback TLS directions | PASS | text-from-peer.txt, text-to-peer.txt; Unicode synthetic text, not Windows. |
| Real Finder Cmd+C / local APFS snapshot / reverse TLS read | PASS | local-source-before-read.json, local-source-tls.txt: metadata first, source overwritten after copy, peer later receives original 4KiB pattern hash. |
| Asynchronous partial pasteboard publication | PASS after fix | pboard rejected two grants after host released scopes. Explicit items + scoped lifetime + Agent independent complete-count verification; appkit-final.txt covers rejection cleanup; release-clipboard.txt and final real Finder hashes verify all3 references. |

## Current machine state

Local functional evidence is recorded; performance acceptance is explicitly failed. The isolated acceptance peer is stopped,
its pairing list is empty, temporary launchd job is booted out, and hidden acceptance domain removed
(release-cleanup.txt). No production launchd installation remains. Signed artifacts and local tar.gz
are retained under target/macos; source and evidence are retained locally; no remote push. The user's
foreground signing session may be stopped with Ctrl-C. No private-key ACL was changed; original
lazy_paste app/domains are untouched.

## Remaining acceptance gate

Physical Windows bidirectional text and 4KiB/64MiB/1GiB Finder/Explorer interop remains BLOCKED:
the user has not started the Windows endpoint. The Windows maintainer can connect using SSH alias
`alarics-macbook-air`; start both Agents and explicitly pair actual device fingerprints before testing.
Full login/reboot, second-user denial, native same-Offer version mutation and cross-version Homebrew
upgrade remain NOT_TESTED. Native failure/restart and install/Homebrew lifecycle evidence from
prior builds remains explicitly scoped; final changes affected host pasteboard publication and its
Agent verification. Developer ID distribution/notarization is not completed.
