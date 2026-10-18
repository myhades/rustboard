# Security model and current limitations

Rustboard treats the LAN as hostile. This development version has not undergone an independent security audit.

## Trust and transport

TLS1.3 is mandatory and TLS1.2 is disabled in the dependency/build configuration. Both sides pin the full DER device certificate using SHA-256, and verify TLS handshake signatures. The sync endpoint accepts only paired pins; there is no trust-on-first-use, plaintext fallback, unauthenticated clipboard endpoint, or automatic pairing. The temporary pair-only TLS endpoint accepts an unpaired certificate only with proof of its private key, displays its fingerprint, and requires explicit confirmation on both devices. It cannot serve clipboard/file data. Device IDs derive from the certificate digest. Pairing is bilateral and requires comparing each displayed fingerprint against the other device. The certificate is a persistent device-key container; public CA chains, DNS names and certificate validity periods are not used as trust policy. Replacing a certificate requires explicit re-pairing.

CLI unpair updates config; restarting applies the change to active trust. Stop immediately disables the service. Core authorization checks the active peer list on every request. A chunk already authorized and in flight may complete. Stopping the service prevents further reads and publishing. The listener address is configured explicitly; isolated tests use loopback and physical acceptance uses the configured LAN endpoints. No firewall/security-policy changes are performed.

Private keys use current-user Windows DPAPI or macOS login Keychain; config contains an encrypted blob or Keychain reference. See docs/CREDENTIAL_STORAGE.md. Same-user malware, administrator/kernel compromise and physical access are outside the current boundary. The current-user Named Pipe DACL and PIPE_REJECT_REMOTE_CLIENTS prevent other ordinary local users/remote clients from controlling the Agent; a second-logon SID boundary and adversarial second-account test remain outstanding. Do not run the Agent elevated.

## Files and resource bounds

Only explicit entries in the current or bounded retained file batches can be read. A paired peer knowing the exact tuple can read a retired batch during its retention window; this is not a single-use transfer authorization. Clients provide opaque identity tuples, never paths. Relative names reject traversal, drive/ADS syntax, device names and unsafe components. Windows rejects reparse ancestors/files, opens regular files and keeps handles with write/delete sharing denied until withdrawal or batch expiry. This may prevent source modifications during an Offer. Local metadata is checked before and after every read. No full hash is computed at copy time.

This is a handle-sharing consistency strategy, not a cryptographic filesystem snapshot. Other platforms must supply a true stable source or invalidate on change. Metadata checks alone cannot detect every hostile timestamp-preserving mutation; privileged or pre-existing memory-mapping writers are not claimed to be covered. A race involving same-user filesystem reconfiguration is not an isolation boundary against that same user.

Limits:1 MiB control frame;4 MiB network Data;64 KiB source/IPC read;256 files;16 peers;8 active server sessions;one incoming latest Offer; at most 8 retained file batches with aggregate encoded metadata below 512 KiB. Text is ≤256 KiB. No clipboard history is kept. Offer-owned image files, APFS stable-source clones and File Provider hydration files are temporary native artifacts. Read/connect/write timeouts bound normal failed operations. A malicious authenticated peer can still consume CPU/connection slots or send frequent clipboard updates; rate limiting and adversarial sustained-load testing are release work. Socket timeouts are per blocking operation, not a complete slowloris-proof total-message budget.

Windows COM source reads can delay the STA up to the callback deadline and a final socket timeout. There is no await on an event loop in the same apartment. All source chunks are bounded, even when COM requests a larger read. No OleFlushClipboard is used. Copies are Copy-only: preferred Move drop effects are rejected.

## Logs, errors, and residual data

Normal diagnostics contain event counts, file sizes, error categories and protocol state, never clipboard text or file contents. Native API errors must not be expanded into source-content dumps. The explicitly invoked loopback acceptance peer writes synthetic test Offers to its test-output folder; it is not a production daemon.

Destination programs own partially written files after failure; Rustboard cannot safely delete files created by arbitrary consumers. Explorer displayed a native read error during the tested pause interruption. Logs distinguish that expected cancellation from successful complete-file transfers. `test-output` contains generated acceptance files and is Git-ignored.

## Reporting and release gates

Do not send keys, real clipboard contents or personal file paths in public issue reports. Include version, platform, sanitized failure category and a synthetic reproducer. Use the repository issue tracker with a sanitized synthetic reproducer; never publish secrets.

Before a release: independent review of unsafe COM/Win32 ownership and pinning; second-user IPC access test; connection flood/slowloris tests; source failure and exit matrix across target apps; long-running soak; broader cross-machine and application coverage. CI files exist but remote CI has not run in this local session.
