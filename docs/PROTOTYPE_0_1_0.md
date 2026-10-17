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

## Pair both ends

Read each installed CLI's `doctor` fingerprint and explicitly pair BOTH sides:

```text
rustboard pair <other-LAN-IP:port> --fingerprint <other-full-SHA256>
rustboard peers
```

Do not use test profile identities. Pairing remains after restart. To change a peer IP, repeat `pair` with its existing fingerprint and new address. There is no automatic discovery, relay or DNS-based address tracking in this prototype.

## Configuration

Windows default profile: `%LOCALAPPDATA%/Rustboard/default/`.
Mac default profile: signed App Group container's `default/` directory, normally `~/Library/Group Containers/<TEAM>.org.rustboard.shared/default/`.

`config.json`:

```json
{"listen":"192.168.100.6:45451","mode":"all","paused":false}
```

Use the machine's own address/port. CLI `--listen` on the Agent overrides the file for that run. Updating listen requires restarting the Agent; rerunning the installer updates installation and startup. A missing address causes bind/start failure rather than silently listening on every interface.

`rustboard pause`, `resume`, `mode text`, `mode all` persist across restart. `pause` is appropriate for temporarily suspending a login-managed Agent. On macOS, `stop` can be restarted by launchd KeepAlive; use `launchctl bootout gui/$(id -u)/org.rustboard.agent` to stop the managed service, or uninstall it.

Identity/private keys and `peers.json` live in the same private profile directory. Do not distribute them in packages. Removing binaries does not remove this data.

Windows logs: `%LOCALAPPDATA%/Rustboard/agent.err.log` for installer-started process. macOS logs: `~/Library/Application Support/Rustboard/agent.err.log`; File Provider stages also appear in unified logging under `org.rustboard`. Windows login Run currently has no redirected console log; use CLI status and a manual diagnostic run when troubleshooting.

## Validation and limits

Release evidence is recorded separately in `docs/PROTOTYPE_RESULTS.md`; prior file-throughput measurements in `docs/PERFORMANCE_LAYERED.md` are historical, not proof of this installed build.

No folder recursion, native inline-image fidelity, automatic updates, public notarization, universal application compatibility, restart-after-OS-reboot guarantee, or 10G performance claim. Previously measured file throughput is not a promise for every network. Clipboard data is not stored as history; retained synthetic test files are not included in packages.
