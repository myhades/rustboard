# Native credential storage and one config

The user-editable profile file is config.json, schema 2. It holds listen, mode, paused, peers and the managed identity block. Certificates are public DER byte arrays. Windows private_key.provider is windows-dpapi with an opaque blob byte array; macOS provider is macos-keychain with an account reference. Never paste a plaintext key into config or copy another machine's identity section.

Windows uses CryptProtectData / CryptUnprotectData with CRYPTPROTECT_UI_FORBIDDEN, current-user scope, no custom password or machine-wide scope. Mac uses SecItemAdd / SecItemCopyMatching for a generic-password item in the default login Keychain, service org.rustboard.device-identity, account equal to the certificate SHA-256 fingerprint, synchronization disabled. The default creator application's Keychain ACL is retained. A duplicate item must contain exactly the same key; it is never overwritten. A noninteractive LAContext avoids background permission dialogs; failures report OSStatus and require the user to resolve Keychain access. No developer signing key is accessed by the Agent.

On startup, after the profile singleton is acquired:

1. Validate settings and legacy peers; load the existing identity or generate one only for a genuinely new legacy-format profile.
2. Protect its private key with the native store and read it back. Validate the certificate/key pair with rustls.
3. Atomically write and sync config.json with the existing identity and peers.
4. Verify any leftover legacy identity matches, then remove identity.json and peers.json. An interrupted cleanup is retried on next startup.

An existing schema-2 config with missing, corrupted or inaccessible credentials fails startup. It never falls back to plaintext or generates a replacement identity. Legacy file removal does not erase prior backups/SSD history. Do not run old Agent builds against a migrated profile. Backing up just config is insufficient to restore a protected identity on another machine.

There is no config watcher or live reload. Manual edits and CLI pair/unpair/pause/resume/mode save desired settings; CLI returns restart_required:true. status and peers describe the running process. Revocation and pause require restart to affect existing connections. stop remains immediate. Packages include scripts/windows/restart.ps1 and scripts/macos/restart.sh for manual application of changes. Runtime clock/lock/socket/cache files are not configuration and remain separate.

Validation: Windows native DPAPI roundtrip/tamper tests; wrong certificate/key rejection; migration with the same fingerprint; restart-only settings; interrupted cleanup; malformed settings and duplicate peers. The installed Windows default profile passed migration and two restart checks. Mac runtime acceptance requires the user-signed Agent in the logged-in launchd session; unsigned SSH helper failure must not be reported as success.

API references:
- https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata
- https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptunprotectdata
- https://developer.apple.com/documentation/security/secitemadd(_:_:)
- https://developer.apple.com/documentation/security/secitemcopymatching(_:_:)
- https://developer.apple.com/documentation/technotes/tn3137-on-mac-keychains
