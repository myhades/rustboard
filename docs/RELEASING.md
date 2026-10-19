# Release automation

Pull requests targeting main run tests, formatting and clippy on Windows, both
macOS architectures and portable Linux code. Branch pushes do not start CI.
Release publication runs tests,
builds, signing and packaging on GitHub-hosted runners.

macOS builds use Apple Development signing without notarization.

## One-time secrets

In `myhades/rustboard`, Settings -> Secrets and variables -> Actions:

* `MACOS_CERTIFICATE_P12_BASE64`: base64 of an exported signing identity, including
  its private key. Export from Keychain Access -> My Certificates -> the existing
  Apple Development identity -> Export as .p12, protected with a strong password.
* `MACOS_CERTIFICATE_PASSWORD`: that export password.
* `MACOS_SIGN_IDENTITY`: the certificate SHA-1 identifier used by sign.sh. Use the
  same signing team as installed versions to retain App Group and Keychain access.

Keep signing credentials out of commits and workflow inputs.
With `gh` authenticated on the Mac, import without printing the certificate:

```sh
base64 -i /absolute/private/path/rustboard-signing.p12 | gh secret set MACOS_CERTIFICATE_P12_BASE64 --repo myhades/rustboard
gh secret set MACOS_CERTIFICATE_PASSWORD --repo myhades/rustboard
gh secret set MACOS_SIGN_IDENTITY --repo myhades/rustboard
```

The last two commands prompt locally; do not add passwords on the command line.
The Mac job creates a temporary keychain, imports this identity, signs, and deletes
the keychain and exported file on exit. No Apple account password is needed and
there is no notarization request. An expired/revoked certificate fails the job.

## Publishing

1. Set Cargo.toml version and update Cargo.lock. Review docs/RELEASE-NOTES.md.
2. Push the tested commit and create its `v<exact Cargo version>` tag. Explicitly
   publish a new GitHub Release for that tag to start release builds. Pushing a
   tag alone does not start release builds.
3. Release builds test and package Windows x64, macOS arm64 and x86_64, and Linux
   x86_64 on Ubuntu 24.04. Windows
   uses Scoop with the existing vendor scripts; a disposable runner tests actual
   bucket install, disabled-service update, identity preservation and uninstall.
4. Only after all builds pass, upload archives, SHA-256 sidecars and generated
   manifests to that Release. The published Release initially has no assets while
   builds run. Published assets are immutable; use a new version to fix them.
5. The distribution job opens a PR updating Casks/rustboard.rb and
   bucket/rustboard.json, then dispatches CI on its branch. Merge the PR after
   checks pass to update both package managers. Repository Actions settings must
   allow GitHub Actions to create pull requests.

For a prerelease, explicitly publish a GitHub prerelease with a tag matching the
Cargo version. Both published releases and prereleases trigger this workflow;
saving a draft does not. There is no tag-push or manual-dispatch release trigger.
Retry failed jobs with GitHub's **Re-run failed jobs**. Asset upload refuses to
overwrite existing assets; if upload partially failed, inspect that state before
retrying. Missing signing Secrets fail the build without uploading binaries or
updating package manifests. The already-published Release remains visible.

Users install Scoop separately from https://scoop.sh/.

```powershell
scoop bucket add myhades https://github.com/myhades/rustboard
scoop install myhades/rustboard
scoop update rustboard
scoop uninstall rustboard
```

Scoop owns its shim; the vendor install runs with -NoPath. Updates preserve the
Agent's enabled/disabled state. Only an actual uninstall removes installed
binaries/startup, and identity/config remain in LocalAppData. No duplicate user
PATH entry is added by the vendor script.

## Linux package

The Linux release job builds the two binaries, packages the vendor scripts, and
writes a SHA-256 sidecar. `release-manifests.py` produces a single cross-platform
Cask: macOS keeps its signed archives and Linux selects the x86_64 tarball. The tap
and Scoop bucket remain in this repository; no extra token or package repository
is required. The published 0.1.0 Cask remains unchanged until a new release supplies
Linux assets. See [Linux installation](LINUX.md) for dependencies and commands.

Linux CI runs `platforms/linux/scripts/test-brew.py` against local release artifacts
and a temporary Cask with local test revision numbers. It checks fresh installation,
actual `brew upgrade`, file replacement, preserved configuration, absence of startup
registration, and uninstall/CLI-link cleanup. These revisions are never published.
The test requires a Brew installation without Rustboard already installed.

For local checks after `cargo build --release --locked --bins`:

```sh
python3 platforms/linux/scripts/package.py
python3 scripts/release-manifests.py --linux-only --tag v0.1.0 --assets target/release-assets --output target/manifests
python3 platforms/linux/scripts/test-brew.py
```

Use the current Cargo version for `--tag`. Package tests do not require a desktop or
unlocked keyring; first-start desktop acceptance is separate from installation.
