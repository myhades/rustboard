Rustboard prototype: text, ordinary files and clipboard images exposed as files.

macOS builds are Apple Development signed, **not notarized**. Installation on an
unrelated Mac, including Open Anyway and File Provider operation, is still awaiting
acceptance. CLI launch on a hosted runner is not a substitute for that test.
Do not disable Gatekeeper or remove quarantine attributes to install this build.

After the Homebrew tap update:

```sh
brew install --cask myhades/tap/rustboard
```

After the Scoop bucket update:

```powershell
scoop bucket add rustboard https://github.com/myhades/scoop-bucket
scoop install rustboard/rustboard
```

Both installers are per-user and manage CLI links/PATH and login startup.
Uninstall retains device identity, paired peers and configuration. Use
`rustboard config`, `rustboard pair` and `rustboard reload` to configure a new
installation. Permissions and manual Open Anyway instructions are in
[INSTALLATION.md](https://github.com/myhades/rustboard/blob/main/docs/INSTALLATION.md).
