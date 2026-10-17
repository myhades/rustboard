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
- Same Offer replacement, expiry, cancellation and authenticated access rules as ordinary files. Default Offer lifetime is ten minutes. Replacing the clipboard can interrupt an ongoing transfer, including replacement via Apple Handoff.

## Install (current user, login startup)

Extract the package into a new directory. Keep its `bin`, `scripts` and app layout. Use a LAN IP actually assigned to the machine; reserve addresses in DHCP for stable pairing. No firewall/security settings are changed by installation.

Windows, PowerShell (replace the IP):

```powershell
./scripts/windows/install.ps1 -Listen 192.168.100.6:45451
& "$env:LOCALAPPDATA/Rustboard/bin/rustboard.exe" status
```

Installs in `%LOCALAPPDATA%/Rustboard/bin`; current-user Run entry `Rustboard`. Agent runs hidden in the logged-in user's session, not Session 0. `scripts/windows/uninstall.ps1` removes startup and binaries but preserves settings/identity.

macOS, from the extracted signed package (replace the IP):

```sh
bash scripts/macos/install.sh --listen 192.168.100.4:45452
"$HOME/Library/Application Support/Rustboard/bin/rustboard" status
```

Installs `~/Applications/Rustboard.app`, binaries in `~/Library/Application Support/Rustboard/bin`, and user LaunchAgent `org.rustboard.agent`. Requires the same real Apple signing team for Agent/CLI/host/extension. The developer's private key is used only by its owner running `scripts/macos/sign.sh`; build/package/install never sign. Signing does not imply notarization. System permission prompts must be handled locally. `bash scripts/macos/uninstall.sh` preserves identity/settings.

Do not assume a successful SSH foreground test proves launchd network/File Provider permission behavior. Installed-background copy must be tested separately. `status` currently confirms reachable Agent/config identity, not complete Finder health.

## macOS file permissions

The installed Agent is a standalone, non-sandboxed executable; the host and File Provider remain sandboxed. File access privacy permissions belong to the installed Agent, not automatically to the visible Rustboard.app. User-approved Files and Folders prompts occurred during acceptance; successful copy did not mean a prompt-free deployment.

For broad file-copy use, the user can grant Full Disk Access in System Settings > Privacy & Security > Full Disk Access. Use + and the file chooser's Cmd+Shift+G to select `~/Library/Application Support/Rustboard/bin/rustboardd`, then enable it. This grants broad read access, including protected app data; it does not make the Agent root or remove File Provider sandbox restrictions. Restart the managed Agent after changing the setting. Do not grant the old scratch-build executable or SSH/Terminal merely to make Rustboard work.

Rustboard cannot silently grant this permission; no installer or script modifies TCC. The user has enabled Full Disk Access; comprehensive cross-folder prompt-free acceptance has not been performed. Pasteboard, network and File Provider permissions remain separate; report the actual prompt if those still appear.

## Pair both ends

Use the installed CLI path shown above, or add its bin directory to your shell PATH.

On the receiving machine run `rustboard pair`. It displays its certificate fingerprint and waits for up to 120 seconds. On the other machine run `rustboard pair <IP>[:port]` (default port 45452). Both terminals show the other device's certificate fingerprint. Compare each against the fingerprint displayed locally on the other machine, and type `y` on BOTH sides. Default/EOF declines. On success, restart both services with `rustboard restart`.

The pairing-only TLS endpoint temporarily uses the configured listening port plus one (e.g. sync 45452, enrollment 45453). That port must be reachable; Rustboard does not alter firewalls. It accepts only identity/confirmation records, never clipboard/file requests. Closing the CLI normally cancels immediately; forced terminal loss closes the window within ten seconds. The total session expires after 120 seconds. TLS proves possession of each displayed certificate's private key; human fingerprint comparison authorizes the identity. No discovery or automatic trust-on-first-use is added.

`rustboard unpair <IP>[:port]` removes the uniquely matching configured peer. Restart to revoke active trust. Unknown/ambiguous addresses fail. Addresses locate peers; certificate fingerprints remain their identities. Repeat mutual pairing after an address change. Interrupted saving may leave one side configured; inspect config or retry, and do not treat an incomplete result as success.

## Daily commands

- `status`: compact running state, login startup, active listen/mode, pending restart and active paired-device list. Offline it shows saved peers. This is not a continuous peer connectivity probe. `status --json` is the machine-readable active state and fails when the Agent is unreachable.
- `start`: immediately starts the installed default service and enables login startup.
- `stop`: immediately stops it and disables login startup, including launchd KeepAlive. It preserves identity/config/pairs.
- `restart`: reloads an enabled service; a disabled service requires `start`.
- `config`: opens config with VISUAL/EDITOR (an executable path), otherwise Windows Notepad or the macOS default text editor. Save and manually restart.
- `pair [IP[:port]]`, `unpair IP[:port]`, `--help`, `--version`.

There are no public pause/resume/mode/peers/doctor commands. Change mode in config. Pair requires a running Agent; config/status/unpair work with the Agent stopped. Service commands only target the installed default profile.

## Configuration

Windows default profile: `%LOCALAPPDATA%/Rustboard/default/`.
Mac default profile: signed App Group container's `default/` directory, normally `~/Library/Group Containers/<TEAM>.org.rustboard.shared/default/`.

Runtime fields in `config.json` (the full generated schema-2 file also contains `schema`, `peers`, and the managed `identity` block; do not replace the full file with this excerpt):

```json
{"listen":"192.168.100.6:45451","mode":"all"}
```

Use the machine's own address/port. CLI `--listen` on the Agent overrides the file for that run. Updating listen requires restarting the Agent; rerunning the installer updates installation and startup. A missing address causes bind/start failure rather than silently listening on every interface.

Configuration edits, pairing and unpairing are saved for the next startup, without live reload. `status` reports pending changes. Run `rustboard restart` after editing. Package restart scripts are wrappers for that command. To immediately revoke access, use `stop` (also disables login startup); a later `start` loads the saved config.

The profile now has ONE editable `config.json` (schema 2), containing `listen`, `mode`, `peers` and `identity`. `peers` is an array of `{ "address": "IP:port", "fingerprint": "64 lowercase SHA-256 hex characters" }`. `identity.certificate_der` contains the public certificate bytes. Keep the automatically managed `identity` section intact.

Windows: `identity.private_key` contains `provider: "windows-dpapi"` and an opaque `blob` byte array, encrypted with current-user DPAPI (not machine-wide scope). macOS: it contains `provider: "macos-keychain"` and `account`, referencing the non-synchronizing login Keychain item under service `org.rustboard.device-identity`. The Agent's signed identity controls Keychain access; this is independent of the developer's signing key. No custom cryptography is implemented. DPAPI is a current-user boundary, not protection against arbitrary malicious code already running as that user.

First startup migrates the old identity and peers without changing the certificate fingerprint. It validates the protected key/certificate pair before atomically committing config, then removes the old `identity.json` and `peers.json`. Storage failure aborts startup without silently generating a replacement identity or falling back to plaintext. Removal does not securely erase SSD blocks, prior backups or old test profiles. Do not run an older Agent against a migrated profile.

A config copy alone is not a portable identity backup: Windows DPAPI recovery needs the corresponding Windows credentials/environment, and Mac needs its Keychain item. Missing/locked credentials cause startup failure; unlock the login Keychain and retry, or explicitly establish a new identity and re-pair. Uninstall retains config and Keychain identity. `clock.json`, locks, sockets and temporary data are runtime state, not additional user configuration.

Windows logs: `%LOCALAPPDATA%/Rustboard/agent.err.log` for installer-started process. macOS logs: `~/Library/Application Support/Rustboard/agent.err.log`; File Provider stages also appear in unified logging under `org.rustboard`. Windows login Run currently has no redirected console log; use CLI status and a manual diagnostic run when troubleshooting.

## Validation and limits

Release evidence is recorded separately in `docs/PROTOTYPE_RESULTS.md`; prior file-throughput measurements in `docs/PERFORMANCE_LAYERED.md` are historical, not proof of this installed build.

No folder recursion, native inline-image fidelity, automatic updates, public notarization, universal application compatibility, restart-after-OS-reboot guarantee, or 10G performance claim. Previously measured file throughput is not a promise for every network. Clipboard data is not stored as history; retained synthetic test files are not included in packages.
