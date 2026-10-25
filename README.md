# Rustboard

[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![Maintainer](https://img.shields.io/badge/maintainer-%40myhades-green)](https://github.com/myhades)
[![Release](https://img.shields.io/github/v/release/myhades/rustboard)](https://github.com/myhades/rustboard/releases)
[![CI](https://github.com/myhades/rustboard/actions/workflows/ci.yml/badge.svg)](https://github.com/myhades/rustboard/actions/workflows/ci.yml)

Native clipboard sync for Windows, macOS and Linux in Rust.

[Installation](#installation) · [Usage](#usage) · [How it works](#how-it-works)

## Highlights

- **Rust implementation.** Shipped in a small CLI and an agent written in Rust.
- **Native clipboard.** Copy, cut, paste directly with OS menus and keyboard shortcuts.
- **Serverless.** Peers communicate directly with each other over TLS.
- **Lazy file transfer.** File transfer triggers only when you paste.
- **Universal Clipboard<sup> [1]</sup>.**  Copy from and to your iDevices.

[1] Requires a macOS peer, see [macOS options](#macos-options) and [Universal Clipboard](#universal-clipboard).

## Demo

https://github.com/user-attachments/assets/817a0642-b6a9-406b-a7d0-f644dc4814a0

## Installation

```sh
# Windows, with Scoop
scoop bucket add myhades https://github.com/myhades/rustboard
scoop install myhades/rustboard

# macOS and Linux, with Homebrew
brew tap myhades/rustboard https://github.com/myhades/rustboard
brew install --cask myhades/rustboard/rustboard
```

Alternatively, download the latest [release](https://github.com/myhades/rustboard/releases/latest), then install it using the bundled scripts under `platforms/<os>/scripts/`.

**For macOS:** Due to lack of notarization, you need to manually allow "rustboard", "rustboardd" and "Rustboard.app" under **System Settings - Privacy & Security** and click **Open Anyway** on macOS, then grant Local Network and Full Disk Access permissions.

**For Linux:** Both X11 and Wayland sessions are supported. Wayland requires GNOME or a desktop implementing ext-data-control-v1, such as a compatible KDE Plasma version. Additionally, `systemd user services`, `FUSE 3` and a Secret Service-compatible `keyring` are required.

To uninstall Rustboard, see [Uninstallation](#uninstallation).

## Usage

### Commands

```sh
# rustboard help
Usage: rustboard <COMMAND> [ARGUMENTS]

Commands:
  status [--json]       Show service state and peers
  start                 Start the service and enable login startup
  stop                  Stop the service and disable login startup
  restart               Restart the service and apply saved configuration
  config                Open the configuration in your text editor
  log                   Open the current log in your text editor
  pair [<IP>[:<PORT>]]  Wait for pairing or connect to a peer
  unpair <IP>[:<PORT>]  Remove pairing for an IP address
  help [<COMMAND>]      Show help for all commands or a specific command
  version               Show the installed CLI version

Run 'rustboard help <COMMAND>' for command details.
```

### Setup

Run `rustboard start` on each computer to initialize Rustboard and enable login startup. Then pair the computers:

```sh
# On the first computer
rustboard pair

# On the second, using the first computer's IP
rustboard pair <IP>[:<PORT>]
```

Compare the fingerprints and confirm on both sides. Peers connect directly, so pair each pair of computers that should exchange clipboard content (see [Multiple peers](#multiple-peers) if you have more than 2 peers).

The default TCP ports are `45452` for sync and `45453` for pairing.

### Use the clipboard sync

Copy as you normally do. Copied content automatically becomes available for pasting on your paired peers.

| Content | Formats | Limit per copy |
| --- | --- | --- |
| Text | Plain text, HTML and RTF. Formats are preserved where the receiving app supports them. | 4 MiB |
| Images | PNG and JPEG. Bitmap formats are converted when needed. | 64 MiB |
| Files and folders | As is. Ownership, ACLs and extended attributes will be lost. | 8 MiB of metadata for up to 10,000 entries |

> [!WARNING]
> Application-specific clipboard formats, symlinks, Windows junctions or special filesystem entries are not supported.

**Cutting / moving files** 

- **Windows / Linux:** <kbd>Ctrl</kbd> + <kbd>X</kbd> (Cut) at the source, then paste at the destination.
- **macOS:** either copy at the source, then <kbd>⌘</kbd> + <kbd>⌥</kbd> + <kbd>V</kbd> (Move Item here) at the destination; or cut at the source, then paste at the destination.

The cutting / moving operation consists of a regular copy and a subsequent step of recycling the original files. Recycling only happens when the paste is completed and recycling is available. This step is skipped otherwise. **Files are never directly deleted.**

**Note that on macOS, completion means File Provider has downloaded every file while Finder may still be writing to the destination.*

### Configuration

Open the config file with `rustboard config`, then run `rustboard restart` to apply changes. The structure of an empty config is as follows.

```json
{
  "listen": "0.0.0.0:45452",
  "mode": "all",
  "peers": [],
  "identity": null
}
```

The first start automatically creates `config.json` and a device identity. Deleting `config.json` regenerates both on the next service start.

#### General

| Setting | Default | Description |
| --- | --- | --- |
| `listen` | `0.0.0.0:45452` | The local listening address. Use a specific IP to restrict the interface, or `null` to disable incoming connections. |
| `mode` | `all` | `all` synchronizes text, images, files and folders. `text` synchronizes plain text only. |

#### macOS

<a id="macos-options"></a>

A set of macOS-specific options to enhance the Universal Clipboard experience. For how this works, check [Universal Clipboard](#universal-clipboard).

| Setting | Default | Description |
| --- | --- | --- |
| `eager_download` | `false` | Automatically download files less than **50 MB** on receipt for Handoff file fetching<sup> [2]</sup>. This will consume bandwidth and cache space even if nothing is pasted. <br> **If disabled, file copying to Universal Clipboard clients will NOT function correctly.** |
| `wake_on_receive` | `false` | Declare user activity on receipt to resume Handoff advertising when idle. This will briefly turn on the display if it was off. <br> **If disabled, Universal Clipboard will NOT update its content when the system is considered idle (usually 2 minutes after the last user input).** |


```json
  "macos": {
    "eager_download": true,
    "wake_on_receive": true
  }
```

[2] The 50 MB limit applies to the whole batch. Cut items are not downloaded in advance.

#### Peers

`peers` stores each peer's address and certificate fingerprint.

#### Identity

`identity` contains the automatically generated certificate and protected private-key data or credential-store reference. Keep it unchanged when editing settings or upgrading to preserve existing pairings.

### Uninstallation

```sh
# Windows, with Scoop
scoop uninstall [--purge] rustboard

# macOS and Linux, with Homebrew
brew uninstall --cask [--zap] myhades/rustboard/rustboard
```

## How it works

### Local and encrypted

There is no central server or relay. Peers exchange clipboard data over direct, mutually authenticated TLS 1.3 connections. Pairing saves the certificate fingerprints confirmed on both computers; subsequent connections must present the matching certificates. Private keys are protected by Windows DPAPI, macOS Keychain, or Linux Secret Service.

### Multiple peers

Each pair connects independently, so A-B and B-C pairings do not establish an A-C connection. Paired computers can receive your shared clipboard content and should be treated as trusted devices.

### File transfers

Copying a file initially sends only metadata. Windows exposes the remote file as an OLE virtual file, macOS uses a File Provider placeholder, and Linux exposes it through a FUSE mount. Reads from the destination application trigger the content fetch from the source in bounded chunks.

An active transfer holds a reference to its batch, so replacing the clipboard does not interrupt it. Rustboard retains the current batch and any older batches still being read, then releases them when no longer needed. Temporary data is cleaned up on exit and leftovers on startup.

### Universal Clipboard

With a macOS peer, content can be passed between Rustboard and Universal Clipboard, so no Rustboard client is needed on iDevices to receive clipboard updates when they're logged into the same iCloud account and have Handoff enabled.

`eager_download` and `wake_on_receive` work around two system limitations: Handoff may read a placeholder without triggering download, leading to a blank paste; and macOS suspends Handoff advertising when the system is idle. These two options enable background file downloads and a brief user-activity declaration, respectively, to make copying to Universal Clipboard clients more reliable.

## Disclaimer

Rustboard is an independent project and is not affiliated with, endorsed by, or sponsored by Apple Inc. Apple, macOS, Handoff and iCloud are trademarks of Apple Inc. Universal Clipboard is an Apple feature referenced here solely to describe interoperability.
