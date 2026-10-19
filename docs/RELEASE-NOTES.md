Rustboard prototype: text, ordinary files and clipboard images exposed as files.

macOS builds are Apple Development signed, **not notarized**. Installation on an
unrelated Mac, including Open Anyway and File Provider operation, is still awaiting
acceptance. CLI launch on a hosted runner is not a substitute for that test.
Do not disable Gatekeeper or remove quarantine attributes to install this build.

After the Homebrew tap update:

```sh
brew tap myhades/rustboard https://github.com/myhades/rustboard
brew install --cask myhades/rustboard/rustboard
```

After the Scoop bucket update:

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
