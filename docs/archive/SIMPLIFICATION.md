# Simplification review — 2026-10-18

The moving/recycling policy experiment was removed at the user's request. Production behavior is ordinary copying, with no sensitive-content option or marker-based filtering. Existing format validation, authentication, bounded reads and retained transfer lifetimes remain.

## Recommended order

1. Move test-only runtime code out of production. Windows clipboard.rs still contains --ole-demo, --sizes, --text-demo and synthetic-source accounting. PatternSource and Identity::load (identity.json) support examples/tests; move them to test support. Production credentials continue using Keychain/DPAPI. This removes alternate paths rather than adding a framework.
2. Consolidate the two thin executable crates into two bin targets in the platform/application crate. Keep rustboard and rustboardd as separate executables: Windows needs a console CLI and a silent GUI-subsystem Agent. Keep the portable core separate for testing and dependency boundaries. Four crates become two without changing process architecture.
3. Group platform-owned resources. A possible structure is crates/core, crates/app/src/{windows,macos}, platforms/macos/{native,scripts,packaging}, platforms/windows/{scripts,packaging}, tests and docs. Today macOS resources span platforms/macos/native, platforms/macos/scripts and platforms/macos/packaging. Moving them is organizational work; update build.rs, packaging, CI and tests together. Do not introduce separate platform crates merely for symmetry.
4. Remove publishing traits if there is still no shared generic caller. ClipboardBackend and FilePresentationBackend currently wrap platform-owned calls; direct platform methods plus the shared PresentedFile/ReadAt data types may be sufficient. Retain ReadAt: streams and snapshots actually share it.
5. Consolidate documentation around one current architecture, protocol, installation and acceptance record. CONTINUE and old results contain obsolete statements; explicitly archive historical evidence, and keep current facts in one place. Avoid adding more status documents per development round.

## Keep the minimum working architecture

Native capture -> Engine (latest clipboard plus bounded retained file batches) -> mutually authenticated TLS metadata/read requests -> native publication. The CLI controls the same Agent over authenticated local IPC. Sources remain lazy and bounded.

Do not combine the Mac host, extension and Agent merely to reduce directory count: File Provider needs its native extension and sandbox integration. Do not remove replay protection, TLS peer verification, native credential storage, stream cancellation, bounds or independent file-batch lifetime. These protect ordinary copy behavior and resource use.

transport.rs currently contains TLS, Node orchestration and portable IPC dispatch. Extracting IPC dispatch from it may improve ownership, but is not itself a code-size reduction. Prefer removing unused production paths first; no async runtime, plugin system, cache database, generic event bus or compatibility layer is needed for this scope.

These are recommendations, not changes made in this round.
