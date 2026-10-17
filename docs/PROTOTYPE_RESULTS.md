# Rustboard 0.1.0 prototype acceptance

## CLI and enrollment follow-up (latest)

- Public CLI consolidated to status/start/stop/restart/config/pair/unpair; --help/--version and status --json. Removed public pause/resume/mode/peers/doctor. Legacy paused config removed on startup; mode remains in config.
- Windows installed service: stop removed current-user Run registration and the process; start restored both; restart preserved enablement and identity. Redirected CLI stdout/stderr no longer stay open in the spawned Agent (explicit standard-handle inheritance guard).
- User-signed macOS installed service: stop disabled launchd and unloaded the job; start enabled and loaded it; restart retained identity. Corrected launchctl print-disabled parsing for both boolean and enabled/disabled output, with a regression test.
- Physical two-device enrollment: unpaired and restarted BOTH installed Agents, opened Mac pairing window, connected Windows CLI, compared displayed fingerprints against independently obtained local identities, confirmed BOTH, saved and restarted BOTH. Original identities and pair addresses restored. Subsequent Win32/AppKit Unicode clipboard sync passed in both directions.
- Windows isolated two-Agent test: decline leaves both configs unchanged; bilateral confirmation saves both; active peers unchanged until restart; address unpair and unknown-address rejection; forced CLI exit closes the temporary listener after heartbeat expiry.
- Pairing TLS test on both OSes verifies certificate possession/identity and rejects normal sync ALPN on the enrollment endpoint. Existing untrusted sync rejection tests remain passing. Pairing uses only TLS 1.3, no new cryptographic algorithm.
- Config editor dispatch: Windows helper received the exact installed config path; Mac VISUAL override dispatched successfully. Windows mode edit showed pending restart, applied only after restart, then restored all. No extra GUI editor usability claim.
- Full workspace tests passed on both platforms (Mac release profile reused cache due disk pressure), plus Windows fmt/Clippy. No full OS reboot, fresh login, new Finder copy benchmark or exhaustive adverse-network enrollment test claimed.

Current packages are under target/prototype/cli. Earlier package manifests and credential/image results below remain historical.


## Credential/config follow-up (latest)

Windows installed Agent has migrated to schema-2 single config and current-user DPAPI, retaining its original fingerprint and one peer. Legacy identity.json and peers.json are removed. Tested pending pause remains inactive until restart, then applies; restored normal operation after another restart. Unit tests cover tampering, wrong certificate/key, interrupted migration cleanup, duplicate peers and malformed config. No plaintext identity fallback.

Mac user-signed Agent is now installed under launchd with schema-2 config and login Keychain storage. Original fingerprint and one peer retained; legacy identity.json / peers.json removed. Pause remained inactive before restart and became active after restart; resume plus a second restart restored normal operation. Both restarts successfully loaded the protected identity. Bidirectional native Unicode clipboard TLS passed with both upgraded Agents. Windows PNG fetched through Mac File Provider: 1,365 bytes, SHA-256 1279f986b08e9c1bba06dbd133a9360414a17408b25e57369eef10dff37fb096. This fetch is not a new Finder GUI acceptance. Earlier unsigned SSH Keychain probe failure -25308 remains historical failed evidence. See CREDENTIAL_STORAGE.md.

The image/file and package results below refer to the earlier prototype installation, before this credential migration.


Date: 2026-10-17. Windows x64 and macOS arm64, same LAN. Evidence below applies to the installed current-user prototype builds, not to an SSH-foreground test Agent. No public release or push was performed.

## Implementation scope

Existing PNG/JPEG/etc. files use the ordinary-file path. Native copied image data is captured as an offer-owned PNG/BMP (Windows) or PNG/TIFF (Mac) and presented remotely as an ordinary file. No image protocol, codec dependency, recursive directory transfer or inline-image publication was added. Encoded image capture is capped at 64 MiB. Temporary sources are removed on release; tests cover withdrawal and rejection.

## Deployment and restart

- Windows: installed under LocalAppData/Rustboard; HKCU Run entry enabled; private default profile; installed Agent running. Installer upgrade retained identity, pause and text-only settings, then normal mode was restored.
- macOS: signed by the user, signature verified; installed under ~/Applications and ~/Library/Application Support/Rustboard; user LaunchAgent enabled; File Provider registered. Agent runs under launchd.
- Both installed default profiles paired using explicit mutual certificate fingerprints. No firewall changes.
- Mac launchctl restart retained identity, peer, paused=true and mode=text. Restored mode=all and paused=false; a fresh Windows PNG was successfully fetched through File Provider after restart.
- This is service-restart evidence, not an OS reboot/login or sleep/wake acceptance result. Login entries are configured; full logout/reboot was not performed.

## Clipboard and image acceptance

- Unicode plain text: synthetic Chinese/emoji text reached both native system clipboards; checked with AppKit and Win32 APIs. Tests allow asynchronous synchronization to settle before checking.
- Windows native PNG -> Mac File Provider: exact bytes and SHA-256 matched; after service restart, 256x256 PNG (1365 bytes) SHA-256 `1279f986b08e9c1bba06dbd133a9360414a17408b25e57369eef10dff37fb096`.
- Windows native DIB -> BMP -> real Finder Cmd+V: user pasted and opened the 256x256 image successfully in Preview. Generated BMP SHA-256 `ba6bdaa2280874feb4697f8fcd01f72344dcc58e35bb3cc775672e1eeb0b28a4`. Windows can synthesize DIBV5 from DIB, so compare the published BMP source with the received BMP, not with a separately wrapped DIB fixture.
- The first fixture was only 2x2 pixels and appeared blank in Preview at original scale. macOS system decoding preserved all four RGBA pixels; user confirmed the larger fixture displays correctly. No runtime image conversion fix was needed.
- Mac native PNG -> real Explorer Paste: SHA-256 matches the PNG above.
- Mac native TIFF -> real Explorer Paste: SHA-256 `4d5ceb8c086bba49d92fa0eec8f71d5fa86ffe9141b6575277a5ef609264b059`, matching the Mac-generated source. Independent decoding of received PNG/TIFF confirmed 256x256 red/green/blue/white quadrants with opaque alpha.
- Existing image files: user copied synthetic.png in Finder; Explorer Paste retained its original name and exact SHA-256. Explorer then copied that received PNG as an ordinary file; Mac File Provider fetched it with the same hash. Both tests ran after the packaged installations were upgraded.
- Both packages were installed from their delivered layouts: Windows using built-in Windows PowerShell 5.1, Mac using the extracted tar archive. Identity and one paired peer were preserved on each side.
- Initial File Provider publication logged a transient domain connection failure; later publication and fetch succeeded. Do not interpret `doctor` alone as Finder health.

## Automated checks

Windows: Rust workspace tests (11 core, 1 compatibility, 6 platform), formatting and clippy all-targets with warnings denied passed. macOS: workspace tests, original vectors, Swift protocol/fault/metadata tests and release app build passed; added image-source ownership test passed (4 platform tests). Signing was performed by the user after the build; subsequent Rust changes were test-only.

Explicit synthetic clipboard fixtures: scripts/windows/prototype-clipboard.py, native/macos/Tests/ImageFixture.swift, native/macos/Tests/ClipboardFixture.swift. These change the system clipboard and are not run as automatic unit tests.

Local raw build/install evidence: test-output/prototype/ on Windows; test-output/prototype-0.1.0/ on Mac. Profiles, certificates, pairing files and test data are excluded from packages.

## Permission qualification (user report and TCC log follow-up)

The user manually approved macOS folder-access prompts during acceptance. At 12:40:01 on 2026-10-17, TCC logged SystemPolicyDocumentsFolder prompting for the installed rustboardd; its preceding SystemPolicyAllFiles preflight was denied. Therefore the transfer checks prove operation after manual consent, not prompt-free access to arbitrary folders. Full Disk Access onboarding and subsequent cross-folder testing remain unverified.

## Limits

No folders/program directories, native inline images, notarization, auto-update, full Homebrew upgrade verification or 10G performance claim. Earlier large-file speed results are in PERFORMANCE_LAYERED.md and were not rerun as a new performance benchmark here. Installed Agent snapshots were approximately 18 MiB Windows working set and 35 MiB Mac RSS; these are idle/small-image observations, not peak memory or CPU benchmarks.
