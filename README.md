# Rustboard

[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![CI](https://github.com/myhades/rustboard/actions/workflows/ci.yml/badge.svg)](https://github.com/myhades/rustboard/actions/workflows/ci.yml)
[![Maintainer](https://img.shields.io/badge/maintainer-%40myhades-green)](https://github.com/myhades)
[![Release](https://img.shields.io/github/v/release/myhades/rustboard)](https://github.com/myhades/rustboard/releases)

Native clipboard sync for Windows, macOS and Linux.

[Installation](#installation) · [Usage](#usage)

## Highlights

- **Rust implementation.** A small CLI and a background agent that work with your system clipboard.
- **Lazy file transfer.** Copy now, transfer when you paste.
- **Apple Universal Clipboard.** Copy texts and files from Windows and Linux to your iPhone and iPad with a Mac. See [macOS eager download](#macos-eager-download).
- **Direct peer connections.** Peers communicate directly with each other in TLS.
- **OS credential storage.** Device private keys are saved in Windows DPAPI, macOS Keychain or Linux Secret Service.

## Demo

<!-- Add the demo GIF at assets/demo.gif, then uncomment the image below. -->
<!-- ![Copy and paste between Windows, macOS and Linux](assets/demo.gif) -->

## Installation

### Windows

Install with Scoop:

```powershell
scoop bucket add myhades https://github.com/myhades/rustboard
scoop install myhades/rustboard
```

### macOS

Install with Homebrew:

```sh
brew tap myhades/rustboard https://github.com/myhades/rustboard
brew install --cask myhades/rustboard/rustboard
```

macOS 13 or later, Apple Silicon or Intel.

The app is development-signed and not notarized, therefore macOS will block its first launch. Navigate to **System Settings - Privacy & Security - Open Anyway**.

Allow Local Network access when prompted. Full Disk Access is only needed to copy files from protected locations.

### Linux

Linux packages will be available with the upcoming 0.1.0 release.

First install the runtime dependencies:

```sh
sudo apt install fuse3 libsecret-1-0 libdbus-1-3 gnome-keyring
```

Then install with Homebrew:

```sh
brew tap myhades/rustboard https://github.com/myhades/rustboard
brew install --cask myhades/rustboard/rustboard
```

## Usage

### Setup

Run `rustboard start` on each computer to initialize Rustboard and enable login startup. Then pair the computers:

```sh
rustboard pair                 # On the first computer
rustboard pair 192.168.1.10     # On the second, using the first computer's IP
```

Compare the fingerprints and confirm on both sides. Peers connect directly, so pair each pair of computers that should exchange clipboard content. The default TCP ports are `45452` for sync and `45453` for pairing.

### Configuration

Open `config.json` with `rustboard config`, then run `rustboard restart` after saving changes. The editable settings are:

```json
{
  "listen": "0.0.0.0:45452",
  "mode": "all",

  "advanced": {
    "macos_eager_download": false
  }
}
```

#### General

| Setting | Description |
| --- | --- |
| `listen` | The local listening address. Defaults to `0.0.0.0:45452`; use a specific IP to restrict the interface, or `null` to disable incoming connections. |
| `mode` | `all` synchronizes text and files, including copied images as files. `text` synchronizes plain text only. |

#### Advanced

<a id="macos-eager-download"></a>

##### `macos_eager_download`

Set `macos_eager_download` to `true` to download incoming file batches smaller than **50 MB** in the background for Universal Clipboard. The limit applies to the entire batch. Larger batches and batches with unknown sizes remain lazy.

This option is disabled by default and consumes bandwidth even if nothing is pasted.

#### Peers

`peers` stores each peer's `address` and certificate `fingerprint`. Use `rustboard pair [IP[:PORT]]` to add a peer and `rustboard unpair IP` to remove one.

#### Identity

`identity` contains the automatically generated certificate and protected private-key data or credential-store reference. Keep it unchanged when editing settings or upgrading to preserve existing pairings.
