# Integration and performance baseline — 2026-10-17

Mac commits `c1c29d2` and `10736fa` were fetched over SSH and fast-forwarded into
the Windows main checkout. No GitHub push. The Mac tracked worktree remains clean;
its signed application under `target/macos` was not rebuilt or signed. Diagnostic
builds run separately under ignored `test-output/performance-probe`.
The imported README was removed in the Windows worktree to retain the user's
explicit no-README repository preference. Its absence is intentional, not a failed gate.

## Windows regression

`platforms/windows/scripts/check.ps1` passed: format, Clippy with warnings denied, all 11
workspace tests, and release build. The tests include the changed shared TLS cache
path and receiver-side unpair rejection. Original Python protocol/hash vectors also
passed. This is not a new Explorer or physical Windows/macOS interoperability run.
Evidence: `test-results/performance/windows-regression.txt`.

## Release diagnostics (64MiB, 64KiB requests)

| Machine / layer | Seconds | MiB/s |
| --- | ---: | ---: |
| Mac Core TLS | 1.126 | 56.83 |
| Mac Node.local_ipc plus JSON serialization | 1.516 | 42.22 |
| Mac production Swift Reply JSONDecoder only | 8.034 | 7.97 |
| Earlier Mac native File Provider fetch from handoff | 23.942 | 2.67 |

Raw results, including Windows measurements, are in `test-results/performance/`.
These are single-run diagnostics, not stable performance thresholds. Mac Core was
rerun using a copy of the repository Cargo.lock (the initial scratch resolution
selected a newer libc and is excluded from the recorded baseline).

The Rust probe uses ephemeral mutually pinned loopback peers, verifies every byte,
checks zero source reads before consumption and exactly one payload's worth of
source reads per pass. Its timers include verification and initial read connection
setup. `local_ipc_json` includes the production Value conversion and serialization,
but **does not include Unix socket traffic, IPC identity validation or Swift**.
The 64MiB reply payload expands to 239,135,526 JSON bytes across 1024 responses.
Mac serialization alone took 0.306 seconds; Value construction is included in the
overall IPC measurement, not the serialization-only timer.

The Swift probe compiles the unchanged production Wire.swift/Security.swift with
`swiftc -O`, repeatedly decodes a 64KiB byte-array Reply, and bounds autoreleases.
It bypasses sockets, signing authentication, TLS, file writes and File Provider.
Average decode time was 7.846ms/chunk. The earlier full fetch averaged about
23.4ms/chunk; decoder cost is significant but does not explain the entire gap.
Do not subtract timings from separate runs and label the remainder socket overhead.

## Reproduce

```sh
cargo run --release --locked -p rustboard-core --example throughput -- 67108864
xcrun swiftc -swift-version 5 -O platforms/macos/native/Security.swift platforms/macos/native/Wire.swift platforms/macos/native/Tests/DecodeThroughput.swift -o /tmp/rustboard-decode-probe
/tmp/rustboard-decode-probe
```

The Windows checkout contains both probe sources. On the Mac the new probes were
copied only into the ignored scratch directory, preserving its committed checkout.
No clipboard changes, domain registration, trust persistence, private-key use,
login jobs or production listeners are required by these probes.

## Next measurements and constraints

1. Profile the existing signed native run: authenticated Unix socket round trip,
   decoding, writes/fsync and File Provider delivery, with CPU/RSS and per-stage time.
2. Measure same-volume APFS clone and local delivery independently of hashing and
   remote hydration. No clone latency or shared-extent measurement was made here.
3. Optimize measured costs within v1, preserving strict parsing, identity checks,
   per-read authorization/version checks, cancellation and failed-output cleanup.
4. Repeat first-fetch Finder hashes/resources/zero pre-read evidence after changes;
   then run actual two-device interoperability in both directions.

Overall performance remains FAIL. No production decoder or protocol change was made.
The current signed build remains usable for further native profiling; rebuilding
the application would require signing again and must respect the user's key-use consent.
