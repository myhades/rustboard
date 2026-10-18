# Current status — 2026-10-18

0.1.0 prototype; Windows/macOS implementation, Ubuntu clipboard backend not implemented.

Copy-only behavior: text, ordinary files and images exposed as ordinary files. No source moving/recycling or sensitive-content option. Both installed device identities and paired certificate fingerprints are retained. Active file reads survive clipboard replacement.

The previous copy-only signed release passed bidirectional native Unicode clipboard checks and a 16-byte Windows source read through the real Mac File Provider with its original retained. Historical Explorer/Finder results live in archive and test-results; they are not evidence for a subsequent refactor.

Current structural refactor: four crates reduced to core/app, standalone test tools extracted, direct platform publication, platform resources regrouped, current docs consolidated. Windows and Mac Rust tests, clippy, release builds, packaging and installation passed. Mac artifacts were signed locally by the user. Both directions passed native Unicode clipboard checks. Windows -> Mac actual File Provider and Mac -> Windows native IDataObject/IStream each transferred a 16-byte synthetic file correctly; no new Finder/Explorer GUI acceptance is claimed. Swift protocol/hydration failure and metadata tests passed. The short-lived AppKit test fixture now materializes file URL data before exiting. Mac signing remains a user-local operation; the agent does not access the user's developer private key. No large-file cross-machine tests authorized.


Repository hygiene: both machines have only main and their primary checkout, with no extra worktree. Mac historical test-output (about 13 GB before cleanup) was removed; two immutable generated fixtures were unlocked only to delete those fixtures. Windows historical test-output deletion was rejected by automatic approval (blocked by policy), so it remains ignored and is not represented as cleaned. Installed credentials/configuration remain outside the workspace and were preserved. Shared/raw historical evidence remains committed under docs/test-results.
