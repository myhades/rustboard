# Clipboard and file-transfer lifetimes

Implementation candidate, 2026-10-18. Windows and Mac automated validation/builds passed; initial user signing and installation completed; small-file physical acceptance has passed as detailed below. The broader concurrency/Handoff release gate remains open.

The current clipboard remains one latest Offer. File batches have a separate bounded registry of metadata and stable source handles. Replacing the clipboard, including with text or unsupported content, retires the presentation without revoking the old batch. Explicit withdrawal/service shutdown remains cancellation. Each read still requires a paired TLS peer, matching source/offer/file/version, valid bounds and a valid stable source. No plaintext transport or previous-format compatibility is added.

A retired batch has a 30-second descriptor-to-read grace period. A validated read renews the whole batch for 120 seconds of idle time, including files not yet requested by a sequential consumer. Active reads can continue past the original Offer presentation TTL. Metadata queries alone do not renew a batch. These are monotonic process-local deadlines; sessions do not survive Agent restart. Retained Offers are not rebroadcast or shown as clipboard history. During the bounded retention window a paired peer with the exact tuple can start another read; this is not a single-use transfer token.

Limits: 8 file batches total, 256 files per batch, aggregate encoded Offer metadata below 512 KiB. No whole-file byte buffering; the existing 4 MiB read window and two-entry IPC connection cache remain. A full registry rejects new file Offers instead of evicting active batches; text remains usable. Completion/cancellation stops requesting bytes, and retired batch handles/clones are reclaimed on idle expiry (not immediately at one file EOF, which could strand other files/readers). Active source replacement or version failure still fails the read. Source handles may keep Windows files locked and APFS clones may retain changed disk blocks until expiry.

Local IPC adds get_file_offer(offer_id) and get_file_offers for retained metadata. read_binary continues to use the existing opaque tuple. Cancellation discards only its request's cached connection, not another concurrent read's cache. Clipboard get_offer still reports only the current item.

Windows IDataObject/IStream retain their original source identities. macOS File Provider resolves an item against its retained batch, not the current clipboard, and checks that batch before/after hydration. Internal batch directories isolate identical names from different Offers; users still paste the original file names. The host's presented-batches.json is a current-format runtime ledger, with no old-ledger import. It evicts only expired batches. Root and working-set changes are signaled after cleanup. The previous single-batch cache must be cleared through the old installed host/domain cleanup before deploying this candidate; no config/identity conversion is involved.

## Verified automatically on Windows

- Core: text replacement at both ends during binary TLS reads; bounded lazy window and explicit withdrawal.
- Batch lifetime: delayed second file, active transfer past presentation expiry, idle reclamation, capacity rejection without eviction, text available at capacity.
- Clock persistence: integer-only old format rejected; current structured state persists replay protection.
- Native COM over real loopback TLS: two 5 MiB + 17 byte files, new file Offer and text replacement during the first stream; second stream requested afterwards; both verified byte-for-byte. This is not an Explorer GUI test.
- Workspace tests and Clippy with warnings denied pass.

## Required physical acceptance

Both directions: start a multi-file copy; replace clipboard with text on source and receiver; start copying a new file batch while the old one continues; use repeated filenames; verify all hashes. Repeat with Handoff enabled, then cancel one copy while another continues. Check no metadata-only body reads, bounded CPU/RSS/handles, idle cleanup, missing source and service-stop behavior. Signed builds and basic Finder requests passed; Finder mid-copy replacement and the remaining concurrency cases are still untested.

Handoff bridge investigation is conditional on those passing. User's proposed deployment rule: no more than one same-Apple-ID Mac with Universal Clipboard enabled in the peer topology. Rustboard cannot infer/verify Apple ID membership from peer certificate pins, and this restriction alone is not loop prevention.

## Metered-link continuation

Mac is reachable over WireGuard at 10.0.2.2. User prohibits large-file testing on the hotspot. Source delta was 101558 bytes, compiled offline on Mac with cached dependencies. Mac Rust tests (13 core + 1 vector + 5 platform), full native build and Swift ProtocolVectors/BinaryFraming/ProviderMetadata passed. No LargeHydration benchmark or large WAN payload was run. Native Windows small_batch_probe is constrained to two 4 KiB named fixtures and pauses after 256 bytes for clipboard replacement. See the small-file results below; broader tests remain pending. Reverse TCP routing to the Windows endpoint timed out; any SSH-forwarded test must be labeled accordingly. No Handoff bridge claim yet.

## First metered physical probe

Mac to Windows native COM probe read its first 256 bytes successfully. Windows then published synthetic text, verified on Mac AppKit. Continuing after a 23-second pause failed with STG_E_READFAULT: the cached TLS socket outlived the server's five-second idle timeout. The client now discards sockets before a new network request when the last request is at least four seconds old, preserving bounded cached bytes and performing normal TLS authentication on reconnect. No failed-request retry or protocol fallback was added. A real TLS regression with a six-second pause and clipboard replacement passes on Windows and Mac. Windows fix is installed; Mac binaries are rebuilt and await user signing. This physical probe is FAIL pending rerun, not a completed acceptance result.

## Metered rerun after idle-connection fix

Both signed/current builds installed with unchanged device and peer certificate pins. Mac -> Windows real out-of-process IDataObject/IStream passed: two 4096-byte fixtures, first 256 bytes read, wait seven seconds, replace Windows clipboard with text and verify text on Mac AppKit, then complete both files byte-for-byte. The second stream is obtained only after replacement. Total body 8192 bytes; this proves native COM consumption across clipboard replacement and idle reconnect, not Explorer GUI acceptance. Evidence: docs/test-results/transfer-lifetime/mac-to-windows-small-batch.json. Windows -> Mac Finder Cmd+V passed: both 4096-byte destination files match the expected bytes and SHA-256; evidence: docs/test-results/transfer-lifetime/windows-to-mac-finder-small.json. Routing includes temporary SSH reverse forwarding, so no throughput claim is made.

## Handoff follow-up (research only)

Apple documents that the general NSPasteboard automatically participates in Universal Clipboard: https://developer.apple.com/documentation/appkit/nspasteboard . Thus the first investigation should use the existing general pasteboard path, not another network transport. Universal Clipboard requires nearby Apple devices with the same Apple Account and Wi-Fi, Bluetooth and Handoff enabled: https://support.apple.com/en-nz/102430 . The Windows-to-Mac WireGuard link does not itself provide that Apple-device proximity. Single participating Mac per Apple-account group remains an operator constraint. Event provenance, marker survival, delayed materialization, echo suppression and native-image-as-file behavior need explicit iPhone/Mac/Windows tests before claiming bridge support. No Handoff feature or private Apple API was added in this change.

## Cleanup and final runtime state

After replacing the tiny-file clipboard with text and waiting for idle expiry, the Mac presented-batches ledger automatically reached zero; evidence: docs/test-results/transfer-lifetime/mac-idle-cleanup.json. This checks native presentation cleanup, not exhaustive disk/handle leak accounting. The temporary SSH reverse tunnel was stopped. Mac must keep its current 10.0.2.2:45452 listener (the old LAN address cannot bind while away); Windows points to that address. Mac's peer endpoint is restored to Windows 192.168.100.6:45451, currently not routable from the Mac. Both services are enabled and identities unchanged. Full bidirectional file use over this hotspot still needs reverse reachability; no persistent tunnel was installed.

## Connectivity correction

A subsequent direct TCP probe from Mac to 192.168.100.6:45451 succeeded in approximately 46 ms, with both installed services responding and no pending configuration restart. The earlier timeout did not establish a missing route; that diagnosis is withdrawn. Current peer addresses point directly at each other, with no SSH forwarding. Earlier forwarded physical-test evidence remains accurately labeled. Handoff has not yet undergone physical acceptance.
