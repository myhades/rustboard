# Security model and current limitations

Rustboard treats the LAN as hostile. This development version has not undergone an independent security audit.

## Trust and transport

TLS1.3 is mandatory and TLS1.2 is disabled in the dependency/build configuration. Both sides pin the full DER device certificate using SHA-256, and verify TLS handshake signatures. The sync endpoint accepts only paired pins; there is no trust-on-first-use, plaintext fallback, unauthenticated clipboard endpoint, or automatic pairing. The temporary pair-only TLS endpoint accepts an unpaired certificate only with proof of its private key, displays its fingerprint, and requires explicit confirmation on both devices. It cannot serve clipboard/file data. Device IDs derive from the certificate digest. Pairing is bilateral and requires comparing each displayed fingerprint against the other device. The certificate is a persistent device-key container; public CA chains, DNS names and certificate validity periods are not used as trust policy. Replacing a certificate requires explicit re-pairing.

CLI unpair updates config; restarting applies the change to active trust. Stop immediately disables the service. Core authorization checks the active peer list on every request. A chunk already authorized and in flight may complete. Stopping the service prevents further reads and publishing. The listener address is configured explicitly; isolated tests use loopback and physical acceptance uses the configured LAN endpoints. No firewall/security-policy changes are performed.

Private keys use current-user Windows DPAPI or macOS login Keychain; config contains an encrypted blob or Keychain reference. See native credential storage below. Same-user malware, administrator/kernel compromise and physical access are outside the current boundary. The current-user Named Pipe DACL and PIPE_REJECT_REMOTE_CLIENTS prevent other ordinary local users/remote clients from controlling the Agent; a second-logon SID boundary and adversarial second-account test remain outstanding. Do not run the Agent elevated.

## Files and resource bounds

Only explicit entries in the current or bounded retained file batches can be read. A paired peer knowing the exact tuple can read a retired batch while a native transfer still owns it; this is not a single-use transfer authorization. Clients provide opaque identity tuples, never paths. Relative names reject traversal, drive/ADS syntax, device names and unsafe components. Windows rejects reparse ancestors/files, opens regular files and keeps fully shared handles during publication. The serving connection acquires a write/delete-denying handle on its first read of a file and releases it on EOF, failure, cancellation or connection timeout (five seconds idle). A later read reacquires the lock and rechecks metadata; publication alone never locks local operations. Local metadata is checked before and after every read. No full hash is computed at copy time.

This is a handle-sharing consistency strategy, not a cryptographic filesystem snapshot. Other platforms must supply a true stable source or invalidate on change. Metadata checks alone cannot detect every hostile timestamp-preserving mutation; privileged or pre-existing memory-mapping writers are not claimed to be covered. A race involving same-user filesystem reconfiguration is not an isolation boundary against that same user.

Limits:1 MiB control frame;4 MiB network Data;64 KiB source/IPC read;256 files;16 peers;8 active server sessions;one incoming latest Offer; at most 8 retained file batches with aggregate encoded metadata below 512 KiB. Text is ≤256 KiB. No clipboard history is kept. Offer-owned image files, APFS stable-source clones and File Provider hydration files are temporary native artifacts. Read/connect/write timeouts bound normal failed operations. A malicious authenticated peer can still consume CPU/connection slots or send frequent clipboard updates; rate limiting and adversarial sustained-load testing are release work. Socket timeouts are per blocking operation, not a complete slowloris-proof total-message budget.

Windows COM source reads can delay the STA up to the callback deadline and a final socket timeout. There is no await on an event loop in the same apartment. All source chunks are bounded, even when COM requests a larger read. No OleFlushClipboard is used. Files are Copy-only: Cut metadata does not request source deletion.

## Logs, errors, and residual data

Normal diagnostics contain event counts, file sizes, error categories and protocol state, never clipboard text or file contents. Native API errors must not be expanded into source-content dumps. The explicitly invoked loopback acceptance peer writes synthetic test Offers to its test-output folder; it is not a production daemon.

Destination programs own partially written files after failure; Rustboard cannot safely delete files created by arbitrary consumers. Explorer displayed a native read error during the tested pause interruption. Logs distinguish that expected cancellation from successful complete-file transfers. `test-output` contains generated acceptance files and is Git-ignored.

## Reporting and release gates

Do not send keys, real clipboard contents or personal file paths in public issue reports. Include version, platform, sanitized failure category and a synthetic reproducer. Use the repository issue tracker with a sanitized synthetic reproducer; never publish secrets.

Before a release: independent review of unsafe COM/Win32 ownership and pinning; second-user IPC access test; connection flood/slowloris tests; source failure and exit matrix across target apps; long-running soak; broader cross-machine and application coverage. CI files exist but remote CI has not run in this local session.

## Native credential storage and one config

The user-editable profile file is config.json, schema 2. It holds listen, mode, peers and the managed identity block. Certificates are public DER byte arrays. Windows private_key.provider is windows-dpapi with an opaque blob byte array; macOS provider is macos-keychain with an account reference. Never paste a plaintext key into config or copy another machine's identity section.

Windows uses CryptProtectData / CryptUnprotectData with CRYPTPROTECT_UI_FORBIDDEN, current-user scope, no custom password or machine-wide scope. Mac uses SecItemAdd / SecItemCopyMatching for a generic-password item in the default login Keychain, service org.rustboard.device-identity, account equal to the certificate SHA-256 fingerprint, synchronization disabled. The default creator application's Keychain ACL is retained. A duplicate item must contain exactly the same key; it is never overwritten. A noninteractive LAContext avoids background permission dialogs; failures report OSStatus and require the user to resolve Keychain access. No developer signing key is accessed by the Agent.

On startup, after acquiring the profile singleton, the Agent validates the current schema-2 config and loads its protected identity from the native store. Certificate/key consistency is checked with rustls. Missing, corrupted or inaccessible stored credentials fail startup; they never trigger plaintext fallback or replacement identity generation.

A new installation starts with the explicit current-format `identity:null` and empty `peers:[]`. Only that initial state creates a new identity, protects it, verifies it and atomically saves config. Existing identity and peers remain unchanged during installation. There is no old-config conversion, legacy identity/peer file import or cleanup, or old cache-format conversion. Missing schema, old schema and unknown keys (including paused) are rejected. Backing up config alone cannot restore a protected identity on another machine.

There is no config watcher or live reload. Interactive mutual pairing and address-based unpair save desired settings; restart applies them. `config` opens the editor, and `status` shows active peers plus pending changes. `start` enables login startup and runs immediately; `stop` disables login startup and stops immediately; `restart` reloads an enabled service. Public pause/resume/mode/peers/doctor commands have been removed. Mode remains configurable; paused is not a config field. The native service manager is the authority for startup enablement, not a second configuration file. Runtime clock/lock/socket/cache files remain separate. Config writes use an OS file lock and atomic replacement (Rust MSRV 1.89).


Validation: native credential roundtrip/tamper tests, certificate/key mismatch rejection, protected identity persistence, current schema validation, restart-only settings and peer save/remove tests. Current installed Windows and user-signed Mac builds retained their existing fingerprints and peers. See PROTOTYPE_RESULTS.md for real-machine evidence. Full OS reboot/login was not tested.

API references:
- https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata
- https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptunprotectdata
- https://developer.apple.com/documentation/security/secitemadd(_:_:)
- https://developer.apple.com/documentation/security/secitemcopymatching(_:_:)
- https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains
