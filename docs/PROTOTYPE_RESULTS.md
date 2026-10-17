# Rustboard 0.1.0 prototype acceptance

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
