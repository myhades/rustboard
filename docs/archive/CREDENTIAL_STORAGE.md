# Native credential storage and one config

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
