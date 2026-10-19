# Linux

Linux supports X11 and GNOME Wayland. Both use Secret Service and the same read-only FUSE mount.
A desktop session and an unlocked keyring are required. Other Wayland compositors are not supported.

The Wayland backend uses Mutter's D-Bus clipboard session without starting screen sharing or
creating input devices. It does not need a GNOME Shell extension or XWayland. This is a
GNOME-specific internal interface, not a compositor-independent Wayland protocol.

Runtime packages on Debian/Ubuntu: `fuse3`, `libsecret-1-0`, `libdbus-1-3`, and a Secret Service
provider such as GNOME Keyring. Build with the Rust toolchain, a C compiler, `pkg-config`
and `libdbus-1-dev`:

```sh
cargo build --release --locked
sh platforms/linux/scripts/install.sh
rustboard start
```

Installation deploys the binaries to `~/.local/lib/rustboard` and the CLI symlink to
`~/.local/bin`. Ensure `~/.local/bin` is on PATH. Installation does not initialize an identity or start the service.

`start` initializes the profile, starts the systemd user service and enables desktop login startup.
`stop` stops it and disables login startup. Configuration is in
`${XDG_CONFIG_HOME:-~/.config}/rustboard/default/config.json`; logs are in the profile's `logs` directory.
The private key lives in Secret Service; the config contains its account reference.

Text, regular files, and PNG/JPEG clipboard images are supported. Images become ordinary files.
Directory sources are not supported. Source files use open descriptors with metadata checks;
Linux advisory locks cannot prevent other applications from modifying them.

Remote files are exposed beneath the profile's `files` mount. Metadata publication does not
read file content. Applications fetch bytes by reading the files; previews and indexers can
also cause reads. No complete downloaded copy is stored. Open files retain their transfer
until closed, including after clipboard replacement. Unused old batches disappear, and
shutdown unmounts the filesystem. The next start removes a disconnected mount left by a crash.

To remove the installation:

```sh
sh platforms/linux/scripts/uninstall.sh
```

Configuration and the keyring identity are retained. No system PATH files are modified.

## Desktop checks

```sh
cargo test --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings
cargo build --release --locked --bins --example clipboard_peer
python3 platforms/linux/scripts/test-desktop.py
```

Run the last command from an X11 desktop session with Thunar, xdotool, Python GI and
GTK 3 installed. It replaces the clipboard with synthetic text, a tiny PNG and
4/8 KiB files. It uses an isolated profile and removes its generated keyring entry.
The report is written to `test-output/linux/desktop.json`. Checks cover X11 INCR in
both directions, Thunar copy/paste, zero content reads before opening files,
active reads across clipboard replacement, and crash recovery.

For an isolated GNOME Wayland check on Ubuntu 24.04, install `mutter`, `wl-clipboard`,
`gnome-keyring` and `python3-gi`, then run:

```sh
python3 platforms/linux/scripts/test-wayland.py
```

This starts a separate headless Mutter session, D-Bus session and temporary keyring. The login
clipboard is not used. Native Wayland clients exercise text and file capture/publication,
large clipboard pipes, image-to-file handling, lazy file reads, replacement during transfer,
and exit cleanup. The report is written to `test-output/linux/wayland.json`.

Platform code is under `src/platform/linux/`; only `clipboard/x11.rs` and `clipboard/gnome.rs`
differ by desktop session. Files, credentials, IPC and service management are shared.
