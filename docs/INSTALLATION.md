# Installation and operation

Scope: paired Windows/macOS machines on a trusted local network. User-session Agent, native clipboard, mutual TLS certificate pins, lazy ordinary-file transfer. This is a prototype, not a notarized/public production release. No README is shipped.

## Supported behavior

- Unicode plain text, up to 256 KiB UTF-8.
- Up to 256 ordinary files in one flat selection, including image files. Directories, application bundles and reparse/symlink sources are unsupported.
- Copying image content produces an ordinary `clipboard-<UUID>.png`, `.bmp` or `.tiff` on the receiving side. Paste into Finder/Explorer to save it. Inline image paste into editors/chat applications is not promised.
- Windows preserves PNG or wraps an uncompressed DIB/DIBV5 in a BMP file. Mac preserves the available PNG/TIFF representation. No pixel decoding/re-encoding library and no new network format are added. Encoded clipboard image data is limited to 64 MiB; unusual compressed/profile-bearing DIBs are rejected.
- Image capture writes a private temporary file locally; its source is deleted when the Offer/source is released. The remote file body is still read on demand. Image capture is not a zero-local-I/O operation.
- Windows keeps offered source handles open without blocking local writes/moves. The first remote read acquires a write/delete-denying handle for that file; EOF, failure, cancellation or connection timeout releases it; active retained batches survive clipboard replacement; their source handles release when the batch expires or is explicitly revoked.
- Mac ordinary-file sources require an APFS clone on the same volume as its private snapshot directory; unsupported volumes and dataless sources are rejected.
- Same Offer replacement, expiry, cancellation and authenticated access rules as ordinary files. Default Offer lifetime is ten minutes. Replacing the clipboard, including via Apple Handoff, retains active file batches; see PROTOCOL.md.

Files always copy; Rustboard never moves, recycles or deletes an original. Cut metadata and sensitive-content markers do not add synchronization policy. Supported text, file and image-file capture retains its normal size and format validation.

## Install (current user, login startup)

Extract the package into a new directory. Keep its `bin`, `scripts` and app layout. Use a LAN IP actually assigned to the machine; reserve addresses in DHCP for stable pairing. No firewall/security settings are changed by installation.

Windows, PowerShell (replace the IP):

```powershell
./platforms/windows/scripts/install.ps1 -Listen 192.168.100.6:45451
& "$env:LOCALAPPDATA/Rustboard/bin/rustboard.exe" status
```

Installs in `%LOCALAPPDATA%/Rustboard/bin`; current-user Run entry `Rustboard`. Agent runs hidden in the logged-in user's session, not Session 0. `platforms/windows/scripts/uninstall.ps1` removes startup and binaries but preserves settings/identity.

macOS, from the extracted signed package (replace the IP):

```sh
bash platforms/macos/scripts/install.sh --listen 192.168.100.4:45452
"$HOME/Library/Application Support/Rustboard/bin/rustboard" status
```

Installs `~/Applications/Rustboard.app`, binaries in `~/Library/Application Support/Rustboard/bin`, and user LaunchAgent `org.rustboard.agent`. Requires the same real Apple signing team for Agent/CLI/host/extension. The developer's private key is used only by its owner running `platforms/macos/scripts/sign.sh`; build/package/install never sign. Signing does not imply notarization. System permission prompts must be handled locally. `bash platforms/macos/scripts/uninstall.sh` preserves identity/settings.

Do not assume a successful SSH foreground test proves launchd network/File Provider permission behavior. Installed-background copy must be tested separately. `status` currently confirms reachable Agent/config identity, not complete Finder health.

## macOS file permissions

The installed Agent is a standalone, non-sandboxed executable; the host and File Provider remain sandboxed. File access privacy permissions belong to the installed Agent, not automatically to the visible Rustboard.app. User-approved Files and Folders prompts occurred during acceptance; successful copy did not mean a prompt-free deployment.

For broad file-copy use, the user can grant Full Disk Access in System Settings > Privacy & Security > Full Disk Access. Use + and the file chooser's Cmd+Shift+G to select `~/Library/Application Support/Rustboard/bin/rustboardd`, then enable it. This grants broad read access, including protected app data; it does not make the Agent root or remove File Provider sandbox restrictions. Restart the managed Agent after changing the setting. Do not grant the old scratch-build executable or SSH/Terminal merely to make Rustboard work.

Rustboard cannot silently grant this permission; no installer or script modifies TCC. The user has enabled Full Disk Access; comprehensive cross-folder prompt-free acceptance has not been performed. Pasteboard, network and File Provider permissions remain separate; report the actual prompt if those still appear.

## Pair both ends

Use `rustboard` in a new terminal after installation, or use the installed CLI path shown above.

On the receiving machine run `rustboard pair`. It displays its certificate fingerprint and waits for up to 120 seconds. On the other machine run `rustboard pair <IP>[:port]` (default port 45452). Both terminals show the other device's certificate fingerprint. Compare each against the fingerprint displayed locally on the other machine, and type `y` on BOTH sides. Default/EOF declines. On success, restart both services with `rustboard restart`.

The pairing-only TLS endpoint temporarily uses the configured listening port plus one (e.g. sync 45452, enrollment 45453). That port must be reachable; Rustboard does not alter firewalls. It accepts only identity/confirmation records, never clipboard/file requests. Closing the CLI normally cancels immediately; forced terminal loss closes the window within ten seconds. The total session expires after 120 seconds. TLS proves possession of each displayed certificate's private key; human fingerprint comparison authorizes the identity. No discovery or automatic trust-on-first-use is added.

`rustboard unpair <IP>[:port]` removes the uniquely matching configured peer. Restart to revoke active trust. Unknown/ambiguous addresses fail. Addresses locate peers; certificate fingerprints remain their identities. Repeat mutual pairing after an address change. Interrupted saving may leave one side configured; inspect config or retry, and do not treat an incomplete result as success.

## Daily commands

- `status`: compact running state, login startup, active listen/mode, pending restart and active paired-device list. Offline it shows saved peers. This is not a continuous peer connectivity probe. `status --json` is the machine-readable active state and fails when the Agent is unreachable.
- `start`: immediately starts the installed default service and enables login startup.
- `stop`: immediately stops it and disables login startup, including launchd KeepAlive. It preserves identity/config/pairs.
- `restart`: restarts an enabled service; a disabled service requires `start`.
- `reload`: applies saved configuration through the same service restart, preserving login startup. It is not a live reload: active clipboard transfers and pairing sessions are interrupted. A disabled service requires `start`.
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

Use the machine's own address/port. CLI `--listen` on the Agent overrides the file for that run. Updating listen requires restarting the Agent; rerunning the installer updates installation and startup. A null listen address disables the listener; an unavailable configured address fails startup rather than silently listening on every interface.

Configuration edits, pairing and unpairing are saved for the next startup, without live reload. `status` reports pending changes. Run `rustboard restart` after editing. Package restart scripts are wrappers for that command. To immediately revoke access, use `stop` (also disables login startup); a later `start` loads the saved config.

The profile now has ONE editable `config.json` (schema 2), containing `listen`, `mode`, `peers` and `identity`. `peers` is an array of `{ "address": "IP:port", "fingerprint": "64 lowercase SHA-256 hex characters" }`. `identity.certificate_der` contains the public certificate bytes. Keep the automatically managed `identity` section intact.

Windows: `identity.private_key` contains `provider: "windows-dpapi"` and an opaque `blob` byte array, encrypted with current-user DPAPI (not machine-wide scope). macOS: it contains `provider: "macos-keychain"` and `account`, referencing the non-synchronizing login Keychain item under service `org.rustboard.device-identity`. The Agent's signed identity controls Keychain access; this is independent of the developer's signing key. No custom cryptography is implemented. DPAPI is a current-user boundary, not protection against arbitrary malicious code already running as that user.

Only current schema-2 config is accepted. A fresh installation initializes an explicit empty identity and empty peer list using the native credential store. Existing protected identity and peers are loaded unchanged. Old config, identity/peer files and cache formats are not imported, converted or cleaned up. Credential failures abort startup without replacement identity generation or plaintext fallback.

A config copy alone is not a portable identity backup: Windows DPAPI recovery needs the corresponding Windows credentials/environment, and Mac needs its Keychain item. Missing/locked credentials cause startup failure; unlock the login Keychain and retry, or explicitly establish a new identity and re-pair. Uninstall retains config and Keychain identity. `clock.json`, locks, sockets and temporary data are runtime state, not additional user configuration.

Windows logs: `%LOCALAPPDATA%/Rustboard/agent.err.log` for installer-started process. macOS logs: `~/Library/Application Support/Rustboard/agent.err.log`; File Provider stages also appear in unified logging under `org.rustboard`. Windows login Run currently has no redirected console log; use CLI status and a manual diagnostic run when troubleshooting.

## Validation and limits

Release evidence is recorded separately in [STATUS.md](STATUS.md); prior file-throughput measurements in [archived performance results](archive/PERFORMANCE_LAYERED.md) are historical, not proof of this installed build.

No folder recursion, native inline-image fidelity, automatic updates, public notarization, universal application compatibility, restart-after-OS-reboot guarantee, or 10G performance claim. Previously measured file throughput is not a promise for every network. Clipboard data is not stored as history; retained synthetic test files are not included in packages.

### macOS small-file eager download (optional)

Add `"macos_eager_download": true` to the top level of `config.json`, then run
`rustboard reload`. The default is false (also when omitted); Windows ignores this
macOS-only setting. Existing identity and peer entries stay unchanged.

Files are always published immediately as File Provider URLs, with no body reads.
When enabled, macOS schedules background downloads only if the entire incoming
batch has known sizes totaling strictly less than 50,000,000 bytes. Otherwise the
whole batch remains lazy. Completion never rewrites the clipboard. Local paste can
request contents immediately through the existing File Provider path. This also
applies to images carried as ordinary files. iPhone-to-Windows transfers use the
existing capture path.

Downloads are scheduled with NSFileProviderManager.requestDownloadForItem, using
the existing File Provider and its expiry/cleanup rules. The system handles the
actual hydration; there is no second cache or transfer protocol. Clipboard polling
continues, and newer clipboard events do not revoke active copies. Scheduling
failure leaves the published files available for normal on-demand reads and logs
the failure. A successful scheduling callback does not mean hydration is complete. iPhone paste should be attempted after download completes;
pasting while a file is still dataless can encounter the existing Handoff issue. With the option disabled, publication
continues to read no file bodies. Enabling it consumes network traffic even when
nothing is pasted. Handoff still depends on Apple's normal device/account setup;
this option makes small files available locally, it does not replace Handoff.


## Distribution: Homebrew Cask and Scoop

GitHub Actions builds version-tagged releases and updates the owned tap/bucket.
See [RELEASING.md](RELEASING.md) for Secrets, build and release instructions.
Both manifests live in this repository: Casks/rustboard.rb and bucket/rustboard.json.
They appear only after the first successful signed release. Do not assume these
commands work before that release.

```sh
brew tap myhades/rustboard https://github.com/myhades/rustboard
brew install --cask myhades/rustboard/rustboard
brew upgrade --cask rustboard
brew uninstall --cask rustboard
```

```powershell
scoop bucket add myhades https://github.com/myhades/rustboard
scoop install myhades/rustboard
scoop update rustboard
scoop uninstall rustboard
```

The Homebrew tap is named myhades/rustboard; the explicit URL avoids its default
lookup of a separate homebrew-rustboard repository. The fully qualified Cask name
is myhades/rustboard/rustboard. On Homebrew 6, installing by this full name trusts
that Cask; later commands can use rustboard. See [tap trust](https://docs.brew.sh/Tap-Trust).
Scoop uses a local bucket alias (myhades here), so myhades/rustboard means
bucket/package. `scoop install rustboard` also works when the name is unambiguous.

Homebrew and Scoop must already be installed. Homebrew owns the CLI link in its
prefix/bin; Scoop owns its shim. Vendor scripts run with PATH modification disabled
under these managers. Direct script installation still manages its own user PATH.
Uninstall preserves identity, pairing and configuration. Service enablement survives
updates. Scoop is per-user only; global installation is rejected.

Windows archives can also be built with
`./platforms/windows/scripts/package.ps1 -OutputRoot target/distribution`.
The shared release-manifests.py generator creates metadata using exact asset hashes.
Never change an archive after its manifest is published.

### macOS permissions and Open Anyway

Allow the normal macOS permission prompt when it first appears. If it was dismissed
or denied, change the permission manually in **System Settings → Privacy & Security**:

- **Full Disk Access**: add the installed Agent at
  `~/Library/Application Support/Rustboard/bin/rustboardd` when access to protected
  files is needed. Text sync alone does not require this broad permission. Reload
  the Agent after granting access.
- **Local Network**: enable the responsible Rustboard process if it was denied.
  Attribution depends on launch context; absence from the list does not prove denial.
  Firewall rules are separate and are not changed by the installer.
- **Open Anyway**: after attempting to launch a blocked component, review the
  Gatekeeper message in Privacy & Security and approve it yourself, then retry.

There is no separate permissions command. Installers do not strip quarantine,
disable Gatekeeper, reset TCC or press approval buttons.

Apple's supported independent distribution route is Developer ID plus notarization.
This prototype deliberately uses Apple Development signing **without notarization**.
Open Anyway does not repair invalid signatures or provisioning restrictions.
Installation and File Provider operation on another clean Mac remain unverified.
CI is authorized to sign using the exported identity in GitHub Secrets; no
notarization credentials or requests are used.

References: [Apple local network privacy](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy),
[Apple safe app opening](https://support.apple.com/102445),
[Homebrew Cask lifecycle](https://docs.brew.sh/Cask-Cookbook),
[Scoop manifests](https://github.com/ScoopInstaller/Scoop/wiki/App-Manifests).

### File Provider cache lifetime

`macos_eager_download` starts background downloads only when every size is known and the
entire batch totals less than 50,000,000 bytes, even without a paste. Exactly
50,000,000 bytes, unknown sizes and larger batches remain lazy. This is a batch
eligibility threshold; it does not bound accumulated File Provider disk use.
There is currently no free-space reserve. Background reads do not take the Windows
transfer lock; detected source rename, move or mutation aborts the download without
a user prompt. Foreground transfers retain their existing locking behavior.

The core retains the current batch and batches owned by active native transfers.
Clipboard replacement immediately drops an old batch without an owner. There is
no descriptor grace period or read-renewal timeout. A live transfer can bridge
idle read-connection reconnects; closing its ownership channel releases the batch.
Windows keeps one batch owner across the presented multi-file data object. macOS
keeps an owner for each File Provider fetch; it has no notification for the lifetime
of an entire Finder paste. A later request after the last owner has ended cannot
revive a discarded batch. Concurrent active batches and metadata remain bounded.

The Mac Agent requests eviction as soon as the retained set changes, retries
failures every ten seconds, and performs a full indexed-cache reset before accepting
new offers on startup and on graceful exit (including SIGTERM). Crash/forced-kill
cleanup runs on next startup. These are cleanup requests, not a promise of immediate
physical disk reclamation by macOS. Active locked partial downloads are never unlinked;
abandoned partial downloads are cleaned at reset or a subsequent fetch.

Files pasted into a destination folder have a separate lifetime: deleting them does
not notify Rustboard to evict its File Provider cache. Successful fetch result files
are handed to the system's File Provider storage management, not manually deleted
behind the framework's back. The batch threshold is not a free-space guarantee.
