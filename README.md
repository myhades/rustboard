# Rustboard

[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
[![Maintainer](https://img.shields.io/badge/maintainer-%40myhades-green)](https://github.com/myhades)
[![CI](https://github.com/myhades/rustboard/actions/workflows/ci.yml/badge.svg)](https://github.com/myhades/rustboard/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/myhades/rustboard)](https://github.com/myhades/rustboard/releases)

Native clipboard sync for Windows, macOS and Linux.

[Installation](#installation) · [Usage](#usage) · [How it works](#how-it-works)

## Highlights

- **Rust implementation.** A small CLI and a background agent that work with your system clipboard.
- **Lazy file transfer.** Copy now, transfer when you paste.
- **Apple Universal Clipboard.** Copy text and files to your iDevices. (requires macOS peer, see [macOS](#macos-options) config)
- **Direct peer connections.** Peers communicate directly with each other over TLS.
- **OS credential storage.** Device private keys are protected by Windows DPAPI, macOS Keychain or Linux Secret Service.

## Demo

https://github.com/user-attachments/assets/817a0642-b6a9-406b-a7d0-f644dc4814a0

## Installation

```sh
# Windows, with Scoop
scoop bucket add myhades https://github.com/myhades/rustboard
scoop install myhades/rustboard

# macOS and Linux, with Homebrew:
brew tap myhades/rustboard https://github.com/myhades/rustboard
brew install --cask myhades/rustboard/rustboard
```

Alternatively, download the latest [release](https://github.com/myhades/rustboard/releases/latest), then install it using the bundled scripts under `platforms/<os>/scripts/`.

Due to lack of notarization, you need to manually allow the app under **System Settings - Privacy & Security** and click **Open Anyway** on macOS, then grant Local Network and Full Disk Access permissions.

## Usage

### Commands

```sh
# rustboard help
Synchronize the clipboard between paired computers.

Usage: rustboard <COMMAND> [ARGUMENTS]

Commands:
  status [--json]        Show service state and peers
  start                  Start the service and enable login startup
  stop                   Stop the service and disable login startup
  restart                Restart the service and apply saved configuration
  config                 Open the configuration in your text editor
  log                    Open the current log in your text editor
  pair [<IP>[:<PORT>]]   Wait for pairing or connect to a peer
  unpair <IP>[:<PORT>]   Remove pairing for an IP address
  help [<COMMAND>]       Show help for all commands or a specific command
  version                Show the installed CLI version

Run 'rustboard help <COMMAND>' for command details.
```

### Setup

Run `rustboard start` on each computer to initialize Rustboard and enable login startup. Then pair the computers:

```sh
# On the first computer
rustboard pair

# On the second, using the first computer's IP
rustboard pair <IP>
```

Compare the fingerprints and confirm on both sides. Peers connect directly, so pair each pair of computers that should exchange clipboard content. The default TCP ports are `45452` for sync and `45453` for pairing.

### Configuration

Open the config file with `rustboard config`, then run `rustboard restart` to apply changes.

```json
{
  "listen": "0.0.0.0:45452",
  "mode": "all"
}
```

#### General

| Setting | Default | Description |
| --- | --- | --- |
| `listen` | `0.0.0.0:45452` | The local listening address. Use a specific IP to restrict the interface, or `null` to disable incoming connections. |
| `mode` | `all` | `all` synchronizes text, images, files and folders. `text` synchronizes plain text only. |

#### macOS

<a id="macos-options"></a>

A set of macOS-specific options to enhance the Universal Clipboard experience.

| Setting | Default | Description |
| --- | --- | --- |
| `eager_download` | `false` | Automatically download files less than **50 MB** on receipt for Handoff file fetching. This will consume bandwidth and cache space even if nothing is pasted. <br> **If disabled, file copying to Universal Clipboard clients will NOT function correctly.** |
| `wake_on_receive` | `false` | Declare user activity on receipt to resume Handoff advertising when idle. This will briefly turn on the display if it was off. <br> **If disabled, Universal Clipboard will NOT update content when the system is considered idle (usually 2 minutes after the last user input).** |

These features are disabled by default. Add the following to your config to enable them.

```json
  "macos": {
    "eager_download": true,
    "wake_on_receive": true
  }
```

#### Peers

`peers` stores each peer's address and certificate fingerprint.

#### Identity

`identity` contains the automatically generated certificate and protected private-key data or credential-store reference. Keep it unchanged when editing settings or upgrading to preserve existing pairings.

## How it works

### Local and encrypted

There is no central server or relay. Peers exchange clipboard data over direct, mutually authenticated TLS 1.3 connections. Pairing saves the certificate fingerprints confirmed on both computers; subsequent connections must present the matching certificates. Private keys are protected by Windows DPAPI, macOS Keychain, or Linux Secret Service.

### Multiple peers

Each pair connects independently, so A-B and B-C pairings do not establish an A-C connection. Paired computers can receive your shared clipboard content and should be treated as trusted devices.

### File transfers

Copying a file initially sends only metadata. Windows exposes the remote file as an OLE virtual file, macOS uses a File Provider placeholder, and Linux exposes it through a read-only FUSE mount. Reads from the destination application trigger the content fetch from the source in bounded chunks.

An active transfer holds a reference to its batch, so replacing the clipboard does not interrupt it. Rustboard retains the current batch and any older batches still being read, then releases them when no longer needed. Temporary data is cleaned up on exit and leftovers on startup.

### Universal Clipboard

A macOS peer passes content between Rustboard and Apple's Universal Clipboard when Handoff is enabled, so no Rustboard client is needed on iPhone or iPad to receive clipboard updates.

The [macOS options](#macos-options) work around two system behaviors: Handoff may read a placeholder without downloading it, and macOS suspends advertising while idle. They enable background file downloads and a brief user-activity assertion to make this work reliably.

## Disclaimer

Rustboard is an independent project and is not affiliated with, endorsed by, or sponsored by Apple Inc. Apple, macOS, and Handoff are trademarks of Apple Inc. Universal Clipboard is an Apple feature referenced here solely to describe interoperability.
