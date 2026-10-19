# 0.1.0 release acceptance — 2026-10-19

Release: https://github.com/myhades/rustboard/releases/tag/v0.1.0
Workflow: https://github.com/myhades/rustboard/actions/runs/39181671997

All release jobs passed: Windows x64, macOS arm64/x86_64, publication and
same-repository manifest updates. The three signing Secrets were accepted;
Apple Development signing and signed CLI launch passed on both macOS runners.
No notarization was performed. No Secrets or private keys are stored here.

## Actual package-manager installation

- Windows: `scoop bucket add myhades https://github.com/myhades/rustboard`, then
  `scoop install myhades/rustboard` downloaded the public ZIP, verified its hash,
  installed the service and created the CLI shim. `scoop update rustboard`
  reported 0.1.0 as current. CLI status, identity, peers and login startup survived.
- Mac: `brew tap myhades/rustboard https://github.com/myhades/rustboard`, then
  `brew install --cask myhades/rustboard/rustboard` downloaded the signed public
  ARM archive. First installation over the existing developer installation failed
  as described below. After recovery and manual Open Anyway approval, rerunning
  the same brew command succeeded, including File Provider registration and CLI
  symlink creation. `brew upgrade --cask rustboard` reported already current;
  `brew outdated --cask rustboard` was empty. CLI status, identity and peers survived.
- Old vendor-managed PATH entries were removed using the existing path scripts.
  CLI entry points are now Scoop's shim and `/opt/homebrew/bin/rustboard`.

These are existing configured machines, not clean-machine acceptance. No actual
cross-version upgrade occurred: 0.1.0 is the first public version. The release CI
separately exercised Scoop upgrade hooks using synthetic version metadata with
identical binaries; it also tested disabled-state preservation and uninstall.
No physical Mac Intel machine or unrelated clean Mac was available. No new Finder
file-paste acceptance is claimed by these installation checks.

## Installer defects found; fix deferred to next release

Directly overwriting the previously executed Agent produced a CODESIGNING
Invalid Page crash despite successful on-disk signature verification. Copying the
same bytes into a fresh inode and replacing the executable removed that failure.
Quarantine was retained. The downloaded Agent then needed the user's Open Anyway
approval; after approval its original identity and pairing loaded successfully.

The main-branch installer now stages each CLI/Agent executable in a fresh file
and renames it into place. Its readiness wait increases from 10 to 30 seconds:
the quarantined program could become ready after the old timeout, causing a false
installation failure. The revised script was tested on the Mac using the public
0.1.0 binaries; installation completed and before/after status matched.

At the user's request, 0.1.0 assets and tag remain unchanged and no 0.1.1 is
published this round. Thus the public 0.1.0 archive still contains the original
installer. The Mac is currently running the public 0.1.0 binaries installed with
the corrected script; Brew records 0.1.0. The fix must be packaged in the next
release before claiming this upgrade defect is fixed for download users.
