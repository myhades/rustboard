# Linux

Linux supports X11, GNOME Wayland, and Wayland compositors exposing `ext-data-control-v1`,
including recent KDE Plasma. All use Secret Service and the same read-only FUSE mount.
A desktop session and an unlocked keyring are required.

Wayland selects the advertised data-control interface on the first seat. GNOME uses Mutter's
D-Bus clipboard session when data-control is absent, without starting screen sharing or
creating input devices. No GNOME Shell extension or XWayland is required. If neither interface
is available, startup reports an error. The older `wlr-data-control` protocol is not used.

## Homebrew

The next release adds Linux x86_64 to the existing tap. Version 0.1.0 has no Linux
release archive. After that release's package manifest is merged:

```sh
brew tap myhades/rustboard https://github.com/myhades/rustboard
brew install --cask myhades/rustboard/rustboard
rustboard start
```

Use current Homebrew with Linux Cask support. The binary requires glibc 2.39 or newer,
systemd user services and a supported desktop session. Install the distribution's
FUSE 3, libsecret, libdbus and Secret Service provider; the Cask does not modify the
system package manager. On Ubuntu/Debian/Kali:

```sh
sudo apt install fuse3 libsecret-1-0 libdbus-1-3 gnome-keyring
```

Homebrew links the CLI into its own `bin` directory. It does not add another PATH
entry or create a second link in `~/.local/bin`. The binaries are installed in
`~/.local/lib/rustboard`. Installation creates no profile, identity or service.

```sh
rustboard stop
brew upgrade --cask myhades/rustboard/rustboard
rustboard start
```

Updates retain configuration and keyring identity. They do not start the service.
To uninstall, run `brew uninstall --cask myhades/rustboard/rustboard`; this stops and
unregisters the service, removes the binaries and Brew CLI link, and retains user data.

## Manual installation

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
python3 platforms/linux/scripts/test-wayland.py --compositor mutter
```

This starts a separate headless Mutter session, D-Bus session and temporary keyring. The login
clipboard is not used. Native Wayland clients exercise text and file capture/publication,
large clipboard pipes, image-to-file handling, lazy file reads, replacement during transfer,
and exit cleanup. The report is written to `test-output/linux/wayland-mutter.json`.

For KWin, install a version exposing `ext-data-control-v1`, plus `dolphin`, `qt6-wayland`,
`wayland-utils`, `wl-clipboard`, `gnome-keyring` and `python3-gi`, then run:

```sh
python3 platforms/linux/scripts/test-wayland.py --compositor kwin
```

This runs the same checks in an isolated virtual KWin session, plus Dolphin's Paste action
and destination content checks. The report is `test-output/linux/wayland-kwin.json`.
The suite has been run with Mutter 46 on Ubuntu 24.04 and KWin 6.6.4 / Dolphin 25.12.3
in an isolated Ubuntu 26.04 environment. Dolphin Copy through keyboard input still needs a
regular desktop session; the virtual test uses native Wayland clients for source capture.

Platform code is under `src/platform/linux/`. The clipboard implementations are `x11.rs`,
`gnome.rs` and `data_control.rs`; both Wayland interfaces share bounded nonblocking pipe IO.
MIME handling, files, credentials, IPC and service management are shared.
