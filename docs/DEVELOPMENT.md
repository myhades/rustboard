# Development

Use a Rust stable toolchain on PATH. macOS additionally requires Xcode command-line tools.

```sh
cargo fmt --all -- --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo build --release --locked
```

Windows: `platforms/windows/scripts/check.ps1`. macOS: `bash platforms/macos/scripts/test.sh`, then `bash platforms/macos/scripts/build.sh`. Tests write generated reports under test-output, never overwrite committed historical evidence. Full vector hashing (`python3 scripts/check-vectors.py --full-hashes`)/large hydration suites are explicit local tests.

## Standalone diagnostics

Rust diagnostic executables live in tools/rust, declared as Cargo examples so they reuse existing dependencies without another package. They are not linked into either installed executable.

- `cargo run -p rustboard --example ole_clipboard -- 4096 30`: Windows virtual clipboard fixture, 4 KiB for 30 seconds; prints reads at exit.
- `cargo run -p rustboard --example throughput -- ...`: synthetic transport benchmark; inspect arguments first.
- Platform stream probes and Mac acceptance peer are separate examples under tools/rust/windows and tools/rust/macos.
- `platforms/windows/scripts/prototype-clipboard.py`: synthetic text/image clipboard producer.
- `tests/support/pattern.rs` is included only by tests/examples; `tests/support/identity.rs` stores only isolated test identities. Installed identities always use native credential stores.

## Contributing

Create a branch and open a pull request against main. Required checks must pass
before merging. Ordinary pushes do not run CI. Release publication runs tests and
builds the distribution archives.

## Test scope

Default checks use bounded synthetic data. Python vectors hash 4 KiB by default; larger hashes are an explicit fixture-integrity check, not transfer acceptance.

- Core: ordering/replay across restart, retained batch lifetime/capacity, tuple authorization, framing bounds and TLS trust/revocation.
- Windows: COM descriptor laziness, stream seek/clone/EOF and a two-file transfer across clipboard replacement; DPAPI and source handle sharing.
- macOS: APFS snapshot lifetime, IPC authorization, Swift wire parsing, hydration failure cleanup and File Provider metadata.

Keep cross-language vector checks: they exercise independent Rust/Swift decoders. Keep the TLS idle-disconnect test despite its six-second wait: it guards the real reconnect-after-clipboard-change regression. A native COM or File Provider test is not redundant with core tests, and is not equivalent to Explorer/Finder GUI acceptance.

Do not add a named scenario unless it changes inputs or exercises a distinct observable behavior. Performance tools, signed live-device checks and large local fixture checks are opt-in; they are not default unit tests. Installed-device identity preservation and bidirectional clipboard checks still require signed on-device acceptance.
