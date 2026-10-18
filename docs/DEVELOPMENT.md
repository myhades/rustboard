# Development

Use a Rust stable toolchain on PATH. macOS additionally requires Xcode command-line tools. There are no hard-coded paths to a personal POC toolchain.

```sh
cargo fmt --all -- --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo build --release --locked
```

Windows: `platforms/windows/scripts/check.ps1`. macOS: `bash platforms/macos/scripts/test.sh`, then `bash platforms/macos/scripts/build.sh`. Tests write generated reports under test-output, never overwrite committed historical evidence. Full vector hashing/large hydration suites are explicit local tests; do not run large-file network tests without authorization.

## Standalone diagnostics

Rust diagnostic executables live in tools/rust, declared as Cargo examples so they reuse existing dependencies without another package. They are not linked into either installed executable.

- `cargo run -p rustboard --example ole_clipboard -- 4096 30`: Windows virtual clipboard fixture, 4 KiB for 30 seconds; prints reads at exit.
- `cargo run -p rustboard --example throughput -- ...`: synthetic transport benchmark; inspect arguments first.
- Platform stream probes and Mac acceptance peer are separate examples under tools/rust/windows and tools/rust/macos.
- `platforms/windows/scripts/prototype-clipboard.py`: synthetic text/image clipboard producer.
- `tests/support/pattern.rs` is included only by tests/examples; `tests/support/identity.rs` stores only isolated test identities. Installed identities always use native credential stores.

No --ole-demo, --text-demo, --sizes or --metrics modes exist in the installed Agent. Use tools and external resource samplers.

## Repository hygiene

Work on main unless isolation is needed; never discard unrelated work. Preserve installed user config/identity and committed evidence when removing build caches or test-output. All developer/AI handoff documentation belongs under docs. Keep current facts in STATUS.md rather than adding another dated continuation file. Historical reports in archive are not operational instructions.
