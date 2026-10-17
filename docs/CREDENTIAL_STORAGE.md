# Native credential storage and one config

The user-editable profile file is config.json, schema 2. It holds listen, mode, peers and the managed identity block. Certificates are public DER byte arrays. Windows private_key.provider is windows-dpapi with an opaque blob byte array; macOS provider is macos-keychain with an account reference. Never paste a plaintext key into config or copy another machine's identity section.

Windows uses CryptProtectData / CryptUnprotectData with CRYPTPROTECT_UI_FORBIDDEN, current-user scope, no custom password or machine-wide scope. Mac uses SecItemAdd / SecItemCopyMatching for a generic-password item in the default login Keychain, service org.rustboard.device-identity, account equal to the certificate SHA-256 fingerprint, synchronization disabled. The default creator application's Keychain ACL is retained. A duplicate item must contain exactly the same key; it is never overwritten. A noninteractive LAContext avoids background permission dialogs; failures report OSStatus and require the user to resolve Keychain access. No developer signing key is accessed by the Agent.

On startup, after the profile singleton is acquired:

1. Validate settings and legacy peers; load the existing identity or generate one only for a genuinely new legacy-format profile.
2. Protect its private key with the native store and read it back. Validate the certificate/key pair with rustls.
3. Atomically write and sync config.json with the existing identity and peers.
4. Verify any leftover legacy identity matches, then remove identity.json and peers.json. An interrupted cleanup is retried on next startup.

An existing schema-2 config with missing, corrupted or inaccessible credentials fails startup. It never falls back to plaintext or generates a replacement identity. Legacy file removal does not erase prior backups/SSD history. Do not run old Agent builds against a migrated profile. Backing up just config is insufficient to restore a protected identity on another machine.

There is no config watcher or live reload. Interactive mutual pairing and address-based unpair save desired settings; restart applies them. `config` opens the editor, and `status` shows active peers plus pending changes. `start` enables login startup and runs immediately; `stop` disables login startup and stops immediately; `restart` reloads an enabled service. Public pause/resume/mode/peers/doctor commands have been removed. Legacy paused fields are removed on startup; mode remains configurable. The native service manager is the authority for startup enablement, not a second configuration file. Runtime clock/lock/socket/cache files remain separate. Config writes use an OS file lock and atomic replacement (Rust MSRV 1.89).


Validation: Windows native DPAPI roundtrip/tamper tests; wrong certificate/key rejection; migration with the same fingerprint; restart-only settings; interrupted cleanup; malformed settings and duplicate peers. The installed Windows default profile passed migration and two restart checks. The user-signed Mac Agent passed installed launchd migration and two service restarts, retaining its fingerprint and peer. Legacy plaintext identity and peer files were removed. Bidirectional Unicode clipboard TLS and Windows PNG to Mac File Provider fetch/hash passed after both migrations. The earlier unsigned SSH helper failed with -25308; it is separate from this successful signed-Agent test. Full OS reboot/login was not tested.

API references:
- https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata
- https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptunprotectdata
- https://developer.apple.com/documentation/security/secitemadd(_:_:)
- https://developer.apple.com/documentation/security/secitemcopymatching(_:_:)
- https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains
