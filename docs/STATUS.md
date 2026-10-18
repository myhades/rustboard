# Current status — 2026-10-19

0.1.0 prototype; Windows/macOS implementation, Ubuntu clipboard backend not implemented.

Copy-only behavior: text, ordinary files and images exposed as ordinary files. No source moving/recycling or sensitive-content option. Both installed device identities and paired certificate fingerprints are retained. Active file reads survive clipboard replacement.

The previous copy-only signed release passed bidirectional native Unicode clipboard checks and a 16-byte Windows source read through the real Mac File Provider with its original retained. Historical Explorer/Finder results live in archive and test-results; they are not evidence for a subsequent refactor.

The earlier structural cleanup extracted standalone test tools, simplified platform publication and consolidated documentation. The current follow-up merges core/app into one root package with two binary targets and platform subdirectories. Windows and Mac Rust tests, clippy, release builds, packaging and installation passed. Mac artifacts were signed locally by the user. Both directions passed native Unicode clipboard checks. Windows -> Mac actual File Provider and Mac -> Windows native IDataObject/IStream each transferred a 16-byte synthetic file correctly; no new Finder/Explorer GUI acceptance is claimed. Swift protocol/hydration failure and metadata tests passed. The short-lived AppKit test fixture now materializes file URL data before exiting. Mac signing remains a user-local operation; the agent does not access the user's developer private key. No large-file cross-machine tests authorized.


Repository hygiene: both machines have only main and their primary checkout, with no extra worktree. Mac historical test-output (about 13 GB before cleanup) was removed; two immutable generated fixtures were unlocked only to delete those fixtures. Windows historical test-output deletion was rejected by automatic approval (blocked by policy), so it remains ignored and is not represented as cleaned. Installed credentials/configuration remain outside the workspace and were preserved. Shared/raw historical evidence remains committed under docs/test-results.

Single-package follow-up completed: one root Cargo package, unchanged CLI/Agent binary names, direct core/app/platform modules. Windows 22 unit tests + 1 vector test and Mac 18 unit tests + 1 vector test passed; format, clippy and release/native builds passed. User-signed Mac and Windows installs passed bidirectional native text and 16-byte file checks (File Provider / IDataObject-IStream). No protocol, config or credential migration was introduced; identities and peers are unchanged. The native stream probe must wait for clipboard publication after metadata arrival; its first early attempt was retried successfully after publication.

Production audit follow-up: removed dormant paused state, unused generic file publication, the Mac ipc-check diagnostic entry, obsolete source counters/cancel state and a forwarding wrapper. Tests now cover persisted replay rejection and invalid file tuples without renewal; redundant Swift hydration scenario removed, large Python hashes opt-in. Windows source publication is fully shared; first remote read acquires a write/delete-denying handle on the same file object. The serving connection releases that handle on source EOF, cancellation, read failure or disconnect (five-second idle timeout), not on destination commit. Windows regression tests cover locking and release over real loopback TLS. Windows 24 unit tests, Mac 19 unit tests and vectors/clippy passed. User-signed Mac installation and both installed Agents passed native Unicode and bidirectional 16-byte file checks; see test-results/production-audit/results.json. No new Explorer/Finder GUI acceptance or large network transfer was performed.


macOS eager-download follow-up (2026-10-19): optional `macos_eager_download`
(default false) keeps immediate file-URL publication and schedules native File
Provider downloads for each file below 50,000,000 bytes. Completion does not
rewrite the clipboard. The user-signed installed Mac has this setting enabled;
identity and pairing were preserved. A fresh 580-byte PNG passed Windows -> Mac
background hydration with matching SHA-256 and direct iPhone Files paste/preview,
confirmed by the user without Finder recopy. Earlier automatic publications left
iPhone showing old content; the cause remains unproven, so this is a successful
single-run acceptance, not a claim of fully established Handoff reliability.
See test-results/handoff-eager/results.json. No large-file transfers were tested.


Distribution preparation (2026-10-19): Homebrew Cask template/generator and Scoop
bucket generator now include native install/uninstall hooks and managed CLI links.
Direct installs manage only owned user PATH entries/stanzas. Both real machines
passed uninstall/reinstall with identity and peers preserved and services restored;
Mac eager-download remains enabled. Homebrew parsed the generated Cask in a
throwaway tap, which was removed. Scoop itself is not installed, so only manifest
JSON and direct vendor-script validation are claimed. Mac signing is still Apple
Development, not Developer ID/notarized. Release assets and public distribution
repositories have not been published. See test-results/distribution/results.json.

Eager-download refinement (2026-10-19): installed on both machines after user
signing. The whole batch must total less than 50,000,000 bytes with all sizes known.
Native requestDownloadForItem produced isSystemRequest=true; a 3,840-byte file
hydrated with the Windows write handle still open and matching SHA-256. With eager
download temporarily disabled, a fresh 3,968-byte file had zero allocated blocks;
a direct on-demand read produced isSystemRequest=false and matching content.
Eager download was restored true. Windows background mutation/rename rejection
and foreground lock lifecycle passed unit/TLS tests, not a cross-machine mutation
race test. Windows 26 unit tests, Mac 19 unit tests, shared vectors, Swift tests,
clippy and both builds passed. No large-file transfers. See
 test-results/eager-batch/results.json. The earlier cache retention policy remains
unchanged; the 50 MB batch threshold is not an accumulated disk quota.

Cache lifecycle (2026-10-19): signed candidate installed on both machines. Only
current and explicitly owned batches remain; 30/120-second history grace removed.
Native tests passed old materialization replacement, graceful Mac stop/reset,
SIGKILL followed by launchd startup cleanup, and Mac-to-Windows two-file COM
transfer across clipboard replacement and data-channel idle reconnect. The second
file stream opened after replacement. Source snapshots fell from two to zero when
the native owner finished. A retained Windows IDataObject reference on incoming
text was found and fixed. Identity, peers, eager setting and autostart preserved.
See test-results/cache-lifetime/results.json for evidence and limits. Earlier
package archives are historical validation artifacts, not releases of this build.

Structural consolidation (2026-10-19): Windows file locking and background
path/change-time validation now live in the Windows source implementation.
Transport is split into concrete TLS, framing, IPC, transfer and pairing modules,
with Node/server orchestration in transport.rs. One package and two binaries;
no new traits, wire formats, compatibility paths or configuration changes.
Windows 26 unit tests + protocol vectors and Mac 19 unit tests + vectors passed;
Mac Python/Swift tests, both clippy checks and release/native builds also passed.
This source-only refactor was not reinstalled or re-signed; the installed signed
cache-lifecycle build and its recorded native acceptance remain unchanged.
CI/CD and Linux implementation are deferred to the next round.
