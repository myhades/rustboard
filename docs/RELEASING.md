# Release automation

Branch pushes and pull requests run tests, formatting and clippy on Windows,
both macOS CPU architectures, and portable Linux code. They do not build release
binaries, package installers, sign code, publish assets or update manifests.
Test executables and native test helpers are compiled only as needed to run tests. No Linux
clipboard backend or package is introduced. Rust is pinned in rust-toolchain.toml
and workflows; dependencies use Cargo.lock. macOS binaries target macOS 13.0.

Release builds use GitHub-hosted runners. Signing does not run on pull requests.
Actions are pinned to commit hashes. Only release publication and manifest updates
have repository write access. Both use the built-in GITHUB_TOKEN; no extra GitHub
tokens or distribution repositories are required.

## Signing status

Apple Development signing can be automated without notarization. This does not
establish that another Mac can load Rustboard's File Provider after Open Anyway.
That acceptance remains pending: there is no second clean Mac available.

The current entitlements use `TEAMID.org.rustboard.shared`, not the iOS `group.`
form. Apple documents that the Team-ID-prefixed form does not need a provisioning
profile: https://developer.apple.com/documentation/xcode/accessing-app-group-containers
Apple's supported independent distribution route is Developer ID:
https://developer.apple.com/documentation/xcode/creating-distribution-signed-code-for-the-mac/
Open Anyway addresses Gatekeeper approval; it is not proof of extension, sandbox
or App Group functionality: https://support.apple.com/en-us/102445

Do not claim universal installation support from the development Mac or a CLI
smoke test. A clean Mac must download the quarantined release through a browser or
Homebrew, approve it through System Settings, register the domain, and paste a
small remote file through Finder. Verify uninstall and CLI availability too.

## One-time secrets

In `myhades/rustboard`, Settings -> Secrets and variables -> Actions:

* `MACOS_CERTIFICATE_P12_BASE64`: base64 of an exported signing identity, including
  its private key. Export from Keychain Access -> My Certificates -> the existing
  Apple Development identity -> Export as .p12, protected with a strong password.
* `MACOS_CERTIFICATE_PASSWORD`: that export password.
* `MACOS_SIGN_IDENTITY`: the certificate SHA-1 identifier used by sign.sh. Use the
  same signing team as installed versions to retain App Group and Keychain access.

Never paste the .p12, password or token into chat, commits, or workflow inputs.
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
3. Release builds test and package Windows x64, macOS arm64 and x86_64. Windows
   uses Scoop with the existing vendor scripts; a disposable runner tests actual
   bucket install, disabled-service update, identity preservation and uninstall.
4. Only after all builds pass, upload archives, SHA-256 sidecars and generated
   manifests to that Release. The published Release initially has no assets while
   builds run. Published assets are immutable; use a new version to fix them.
5. Commit Casks/rustboard.rb and bucket/rustboard.json to main in this repository
   using GITHUB_TOKEN. Only these two generated files are staged; source and release
   tags are unchanged. No community review, WinGet or separate EXE installer.

For a prerelease, explicitly publish a GitHub prerelease with a tag matching the
Cargo version. Both published releases and prereleases trigger this workflow;
saving a draft does not. There is no tag-push or manual-dispatch release trigger.
Retry failed jobs with GitHub's **Re-run failed jobs**. Asset upload refuses to
overwrite existing assets; if upload partially failed, inspect that state before
retrying. Missing signing Secrets fail the build without uploading binaries or
updating package manifests. The already-published Release remains visible.

The main repository serves both managers. Do not add manifests with placeholder
URLs/hashes. The first successful release creates Casks/rustboard.rb and
bucket/rustboard.json on main. Repository rules must permit the workflow bot to
push these updates; if a push fails, rerun the distribution job after resolving it.
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
