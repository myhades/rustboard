Rustboard 0.2.0: text, ordinary files and clipboard images exposed as files.

- Linux support for X11, GNOME Wayland and Wayland data-control, with Homebrew packaging.
- Streamed TLS file responses and reusable receive buffers.
- Pairing synchronizes clipboard sequence counters and supports cancellation with Ctrl+C.
- Simplified CLI output and a `log` command that opens the current Agent log.
- Advanced settings are grouped under `advanced` in `config.json`.

Before upgrading, run `rustboard stop`. Move the existing top-level
`macos_eager_download` setting into `advanced.macos_eager_download`, keeping
`peers` and `identity` unchanged. Install the update, then run `rustboard start`.
When pairing again, both peers must use the updated pairing implementation.

macOS builds are Apple Development signed, **not notarized**.

Homebrew:

```sh
brew tap myhades/rustboard https://github.com/myhades/rustboard
brew install --cask myhades/rustboard/rustboard
```

Scoop:

```powershell
scoop bucket add myhades https://github.com/myhades/rustboard
scoop install myhades/rustboard
```

Both installers only deploy files and CLI links/PATH. They do not initialize
configuration, identity, platform services or login startup.
Uninstall retains device identity, paired peers and configuration. Use
`rustboard start` first, then `rustboard config`, `rustboard pair` and `rustboard restart` to configure a new
installation. Permissions and manual Open Anyway instructions are in
[INSTALLATION.md](https://github.com/myhades/rustboard/blob/main/docs/INSTALLATION.md).
