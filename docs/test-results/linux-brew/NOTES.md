# Linux Homebrew checks

Tested on Kali x86_64, glibc 2.40, Xfce/X11 and systemd user services.
Homebrew commit: `41f40decfa0789cff312806606757300bb8bd1c3`.
The package was built on Ubuntu 24.04 using the release build profile.

Homebrew ran from an isolated user-owned prefix under `test-output/archive-20261019/linux-brew/brew`.
No system-wide Brew installation or shell PATH changes were made. Tests used local
archives and Cask revisions `0.1.0-test.1` and `0.1.0-test.2`; no tag, GitHub Release
or public package manifest was published. Both revisions contain the same application
version; the package update checks file replacement and lifecycle behavior.

The final combined macOS/Linux Cask passed `brew readall --os=all --arch=all` and
actual Linux install, upgrade and uninstall. macOS artifact hashes in the isolated
manifest-generation fixture represent test files, not distributable macOS archives;
only the Linux artifact was installed. No macOS installation was tested here.

- `brew.json`: installation creates no profile or startup files; upgrade replaces a
  stale binary and preserves configuration byte-for-byte; uninstall removes the CLI
  link and binaries while retaining configuration.
- `desktop-result.json`: installation and pre-start pairing do not initialize;
  first `start` creates a Secret Service identity and enables the user service;
  upgrade preserves that identity and leaves the service stopped; a subsequent
  `start` succeeds; uninstall of the running service stops it and removes startup
  registration while retaining configuration.

Desktop testing backed up and restored the original Kali installation and profile.
The original service was inactive and remains inactive, without startup registration.
The three identities generated during the test attempts were removed from Secret
Service; the original identity was retained.

The desktop check exposed Homebrew's removal of session environment variables from
uninstall hooks. The Linux uninstall script now supplies the standard user runtime
directory before controlling systemd. Uninstall before first start does not contact
systemd or initialize application state.

Python compilation, workflow YAML parsing, checksum verification and Windows-only
manifest generation also passed. The Release workflow itself was not dispatched.
Raw logs and the desktop driver remain under `test-output/archive-20261019/linux-brew/` on Kali.
