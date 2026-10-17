# Rustboard 0.1.0 prototype

Scope: paired Windows/macOS machines on a trusted local network. User-session Agent, native clipboard, mutual TLS certificate pins, lazy ordinary-file transfer. This is a prototype, not a notarized/public production release. No README is shipped.

## Supported behavior

- Unicode plain text, up to 256 KiB UTF-8.
- Up to 256 ordinary files in one flat selection, including image files. Directories, application bundles and reparse/symlink sources are unsupported.
- Copying image content produces an ordinary `clipboard-<UUID>.png`, `.bmp` or `.tiff` on the receiving side. Paste into Finder/Explorer to save it. Inline image paste into editors/chat applications is not promised.
- Windows preserves PNG or wraps an uncompressed DIB/DIBV5 in a BMP file. Mac preserves the available PNG/TIFF representation. No pixel decoding/re-encoding library and no new network format are added. Encoded clipboard image data is limited to 64 MiB; unusual compressed/profile-bearing DIBs are rejected.
- Image capture writes a private temporary file locally; its source is deleted when the Offer/source is released. The remote file body is still read on demand. Image capture is not a zero-local-I/O operation.
- Windows keeps offered source files open with write/delete sharing denied until the Offer is released; copying a different item releases the previous source.
- Mac ordinary-file sources require an APFS clone on the same volume as its private snapshot directory; unsupported volumes and dataless sources are rejected.
- Same Offer replacement, pause, expiry, cancellation and authenticated access rules as ordinary files. Default Offer lifetime is ten minutes. Replacing the clipboard can interrupt an ongoing transfer, including replacement via Apple Handoff.

## Install (current user, login startup)

Extract the package into a new directory. Keep its `bin`, `scripts` and app layout. Use a LAN IP actually assigned to the machine; reserve addresses in DHCP for stable pairing. No firewall/security settings are changed by installation.

Windows, PowerShell (replace the IP):

```powershell
./scripts/windows/install.ps1 -Listen 192.168.100.6:45451
& "$env:LOCALAPPDATA/Rustboard/bin/rustboard.exe" doctor
```

Installs in `%LOCALAPPDATA%/Rustboard/bin`; current-user Run entry `Rustboard`. Agent runs hidden in the logged-in user's session, not Session 0. `scripts/windows/uninstall.ps1` removes startup and binaries but preserves settings/identity.

macOS, from the extracted signed package (replace the IP):

```sh
bash scripts/macos/install.sh --listen 192.168.100.4:45452
"$HOME/Library/Application Support/Rustboard/bin/rustboard" doctor
```

Installs `~/Applications/Rustboard.app`, binaries in `~/Library/Application Support/Rustboard/bin`, and user LaunchAgent `org.rustboard.agent`. Requires the same real Apple signing team for Agent/CLI/host/extension. The developer's private key is used only by its owner running `scripts/macos/sign.sh`; build/package/install never sign. Signing does not imply notarization. System permission prompts must be handled locally. `bash scripts/macos/uninstall.sh` preserves identity/settings.

Do not assume a successful SSH foreground test proves launchd network/File Provider permission behavior. Installed-background copy must be tested separately. `doctor` currently confirms reachable Agent/config identity, not complete Finder health.

## macOS file permissions

The installed Agent is a standalone, non-sandboxed executable; the host and File Provider remain sandboxed. File access privacy permissions belong to the installed Agent, not automatically to the visible Rustboard.app. User-approved Files and Folders prompts occurred during acceptance; successful copy did not mean a prompt-free deployment.

For broad file-copy use, the user can grant Full Disk Access in System Settings > Privacy & Security > Full Disk Access. Use + and the file chooser's Cmd+Shift+G to select `~/Library/Application Support/Rustboard/bin/rustboardd`, then enable it. This grants broad read access, including protected app data; it does not make the Agent root or remove File Provider sandbox restrictions. Restart the managed Agent after changing the setting. Do not grant the old scratch-build executable or SSH/Terminal merely to make Rustboard work.

Rustboard cannot silently grant this permission; no installer or script modifies TCC. Full Disk Access has not yet been granted/retested in this acceptance record. Pasteboard, network and File Provider permissions remain separate; report the actual prompt if those still appear.

## Pair both ends

Read each installed CLI's `doctor` fingerprint and explicitly pair BOTH sides, then manually restart BOTH Agents:

```text
rustboard pair <other-LAN-IP:port> --fingerprint <other-full-SHA256>
rustboard peers
```

Do not use test profile identities. Pairing remains after restart. To change a peer IP, repeat `pair` with its existing fingerprint and new address. There is no automatic discovery, relay or DNS-based address tracking in this prototype.

## Configuration

Windows default profile: `%LOCALAPPDATA%/Rustboard/default/`.
Mac default profile: signed App Group container's `default/` directory, normally `~/Library/Group Containers/<TEAM>.org.rustboard.shared/default/`.

Runtime fields in `config.json` (the full generated schema-2 file also contains `schema`, `peers`, and the managed `identity` block; do not replace the full file with this excerpt):

```json
{"listen":"192.168.100.6:45451","mode":"all","paused":false}
```

Use the machine's own address/port. CLI `--listen` on the Agent overrides the file for that run. Updating listen requires restarting the Agent; rerunning the installer updates installation and startup. A missing address causes bind/start failure rather than silently listening on every interface.

All config changes, including CLI `pair`, `unpair`, `pause`, `resume` and `mode`, are saved only and return `restart_required: true`. They do not alter the running Agent or existing connections. `status`/`peers` show ACTIVE state; inspect config for desired state. Manually restart after editing:

- Windows: `./scripts/windows/restart.ps1` from the package.
- Mac: `bash scripts/macos/restart.sh` from the package, or `launchctl kickstart -k gui/$(id -u)/org.rustboard.agent`.

To revoke a peer or pause immediately, stop/restart the Agent after saving; saving alone does not disconnect it. On Mac, `stop` may be restarted by launchd KeepAlive; use `launchctl bootout gui/$(id -u)/org.rustboard.agent` to keep it stopped.

The profile now has ONE editable `config.json` (schema 2), containing `listen`, `mode`, `paused`, `peers` and `identity`. `peers` is an array of `{ "address": "IP:port", "fingerprint": "64 lowercase SHA-256 hex characters" }`. `identity.certificate_der` contains the public certificate bytes. Keep the automatically managed `identity` section intact.

Windows: `identity.private_key` contains `provider: "windows-dpapi"` and an opaque `blob` byte array, encrypted with current-user DPAPI (not machine-wide scope). macOS: it contains `provider: "macos-keychain"` and `account`, referencing the non-synchronizing login Keychain item under service `org.rustboard.device-identity`. The Agent's signed identity controls Keychain access; this is independent of the developer's signing key. No custom cryptography is implemented. DPAPI is a current-user boundary, not protection against arbitrary malicious code already running as that user.

First startup migrates the old identity and peers without changing the certificate fingerprint. It validates the protected key/certificate pair before atomically committing config, then removes the old `identity.json` and `peers.json`. Storage failure aborts startup without silently generating a replacement identity or falling back to plaintext. Removal does not securely erase SSD blocks, prior backups or old test profiles. Do not run an older Agent against a migrated profile.

A config copy alone is not a portable identity backup: Windows DPAPI recovery needs the corresponding Windows credentials/environment, and Mac needs its Keychain item. Missing/locked credentials cause startup failure; unlock the login Keychain and retry, or explicitly establish a new identity and re-pair. Uninstall retains config and Keychain identity. `clock.json`, locks, sockets and temporary data are runtime state, not additional user configuration.

Windows logs: `%LOCALAPPDATA%/Rustboard/agent.err.log` for installer-started process. macOS logs: `~/Library/Application Support/Rustboard/agent.err.log`; File Provider stages also appear in unified logging under `org.rustboard`. Windows login Run currently has no redirected console log; use CLI status and a manual diagnostic run when troubleshooting.

## Validation and limits

Release evidence is recorded separately in `docs/PROTOTYPE_RESULTS.md`; prior file-throughput measurements in `docs/PERFORMANCE_LAYERED.md` are historical, not proof of this installed build.

No folder recursion, native inline-image fidelity, automatic updates, public notarization, universal application compatibility, restart-after-OS-reboot guarantee, or 10G performance claim. Previously measured file throughput is not a promise for every network. Clipboard data is not stored as history; retained synthetic test files are not included in packages.
