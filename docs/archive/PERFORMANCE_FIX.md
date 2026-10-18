# Data-path performance fixes — 2026-10-17

Two production decoding changes and typed-IPC responses are implemented and validated,
including signed Mac Finder and physical Windows Explorer ↔ Mac Finder three-file
acceptance. See PHYSICAL_INTEROP.md for the later native run, interruptions and
runtime-context limitation. Near-link-speed performance remains unachieved.
No wire protocol, shared vectors, pairing, authorization,
source lifetime or cache policy changed. This round is closed with these limits;
the next performance project is specified in PERFORMANCE_TARGETS.md.

## Changes and measured causes

1. Rust `RemoteSource` now decodes an expected Data response directly into a strict
   typed struct. The previous internally tagged `Message` deserializer buffered an
   intermediate value for every byte in the numeric array. The new path retains
   rejection of wrong message types/unknown fields, request ID, offset, byte count,
   EOF and revocation checks. An error frame still fails the read and drops the connection.
2. Swift `decodeReply` recognizes the exact compact, sorted Data envelope emitted
   by `Node.local_ipc` and scans bounded decimal integers directly into `[UInt8]`.
   It checks byte range, leading zeros, UInt64 overflow, separators and the complete
   envelope. Other field ordering/spacing/escaped layouts retain JSONDecoder.
   Canonical byte-array syntax errors are rejected rather than sent to Foundation's
   permissive trailing-comma parser. Production IPC authentication and hydrate's
   version/cancel/EOF/cleanup checks are unchanged.

Both changes remove per-byte general-purpose decoding work. Neither prefetches
content nor stores whole files in RAM. The maximum read remains 64KiB.

## Measurements

| Scope | Before | After |
| --- | ---: | ---: |
| Windows Core TLS, 64MiB | 21.17MiB/s | 47.27MiB/s |
| Windows Node.local_ipc + JSON, 64MiB | 14.92MiB/s | 25.17MiB/s |
| Mac Swift decode only, 64MiB | 8.034s | 0.213s |
| Mac diagnostic TLS → Unix → Swift → fsync, 64MiB | 9.551s | 1.548s |

The last comparison uses the **same optimized Rust server** on both sides, changing
only the Swift decoder (`--generic` vs default). It isolates the Swift improvement;
it is not a before/after comparison of the earlier signed Finder run. The same server
spent about 0.80s in Node and 0.49s serializing/writing in each 64MiB run, while Swift
decode fell from 8.205s to 0.219s.

Large-file after measurements:

- Mac diagnostic pipeline: 1GiB in 25.388s (40.33MiB/s), independent SHA256 matches.
  Source read exactly 1GiB. `/usr/bin/time -l`: Swift max RSS 11,141,120 bytes,
  4.01s user + 1.65s system CPU; Core server (both TLS peers in one process) max RSS
  9,273,344 bytes, 19.01s user + 1.88s system CPU. Client resource timing includes
  post-transfer hashing; the reported hydration duration excludes that hash.
- Windows real IDataObject/IStream → file + sync: 1GiB in 19.262s (53.16MiB/s).
  Source read exactly 1GiB, zero publication reads, independent PowerShell SHA256
  matches shared vectors. 50ms samples: max working set 13,717,504 bytes, sampled
  CPU 20.3125s (source and receiver threads in the same process).
- Both platforms also passed 4KiB and 64MiB content/hash checks. Small-file timings
  include connection setup and should not be interpreted as steady-state throughput.

These are diagnostic runs, not a statistically stable performance guarantee. Windows
IStream is in-process and does not measure Explorer or out-of-process COM marshaling.
Mac Unix benchmark uses a private, ephemeral **test endpoint**, not signed Agent IPC
or File Provider. It uses the real Core, v1 framing and production hydration code;
it is not an authentication test and introduces no production bypass.

Background scheduling sensitivity was separately reproduced with `taskpolicy -b`:
the old Swift decoder took 39.651s for 64MiB, optimized decoding 1.228s. This supports
profiling runtime scheduling as well as CPU work; it does **not** prove the original
Finder extension ran under that policy. No production QoS was changed to inflate results.

## APFS measured separately

`profile-clone.py` materialized actual pattern files before timing, then called
`clonefile` for source→snapshot and snapshot→delivery. No byte-copy fallback exists.
At 1GiB the calls took 0.149ms and 0.088ms. Changing the source afterward left both
clone SHA256 values intact. Hashing and source creation/fsync are outside clone timers.
This is COW primitive evidence, not File Provider end-to-end handoff or physical
extent mapping. The earlier 383s remote first fetch was never a clone measurement.

## Validation and reproduction

- Windows: format, Clippy `-D warnings`, 12 Rust tests, release workspace build,
  original Python vectors, native COM stream and independent file hashes.
- Mac isolated checkout: format, Clippy, 11 Rust tests, Python vectors, Swift shared
  protocol/fault cleanup tests, new fast/fallback decoder tests, provider metadata,
  full release app/extension build. Tests include truncated/malformed replies,
  UInt64 precision, unknown fields, cancellation and abandoned-file cleanup.
- Shared `PROTOCOL.md` and `tests/protocol/vectors.json` unchanged.

```powershell
cargo build --release --locked -p rustboard-app --example stream_throughput
./platforms/windows/scripts/profile-stream.ps1 -OutputDirectory ./test-output/new-stream-run
```

```sh
cargo build --release --locked -p rustboard-core --example throughput
xcrun swiftc -swift-version 5 -O platforms/macos/native/Security.swift platforms/macos/native/Wire.swift platforms/macos/native/Tests/SocketHydration.swift -o /tmp/rustboard-socket-hydrate
python3 platforms/macos/scripts/profile-hydration.py target/release/examples/throughput /tmp/rustboard-socket-hydrate tests/protocol/vectors.json test-output/new-hydration-run
python3 platforms/macos/scripts/profile-clone.py tests/protocol/vectors.json
```

Use `--generic --sizes 67108864` and a different new output directory for the Swift
comparison. Raw evidence: `docs/test-results/performance/fixed/`.

## Native follow-up state

Mac corrected sources/build are staged at:
`/Users/myhades/Documents/GitHub/rustboard/test-output/performance-probe/repo`.
The main Mac tracked checkout was not overwritten. Its original signed application
was not rebuilt by this work. User chose to sign locally; no personal key operations
were performed by the agent. The first user signing notification signed the original
`target/macos`, not this staged directory; staged verification failed and no native
test Agent/domain was started at that point. Subsequent native results below supersede
that signing blocker. No GitHub push.

## Signed native validation completed (first two fixes)

The user signed the corrected staged build after unlocking the keychain in their
own SSH session. Strict signature verification passed. macOS initially selected
the original provider, so the test was stopped before reading payload, its domain
removed, and provider registration switched to the staged extension. Runtime paths
and signed binary SHA256 are recorded in `performance/native-fixed/build.json` and
resource evidence. No private-key operation was performed by the agent.

Real File Provider hydration initiated by programmatic read/copy:

- 64MiB fetch callback: 5.498s; read/copy/fsync: 6.500s.
- 1GiB fetch callback: 88.618s; read/copy/fsync: 99.089s.
- All three sizes matched independent SHA256. Initial items were dataless with
  zero blocks and zero source bytes read; final source reads exactly equalled size.

Then a NEW Offer was published and zero-read/dataless state recorded again. The
user performed real Finder Cmd+V into the empty `test-output/finder-fixed` directory:

| Size | Original Finder fetch | Corrected Finder fetch |
| --- | ---: | ---: |
| 4KiB | — | 0.060s |
| 64MiB | 23.942s | 3.593s |
| 1GiB | 383.093s | 58.055s |

These intervals are fetch_start→fetch_complete, not total Finder UI time. The new
1GiB callback is about 6.6x faster than the historical run (~17.64MiB/s). All three
actual Finder destination hashes match shared vectors; source counters equal size,
so this was first hydration, not a cache-hit performance claim. Historical and new
runs were not a controlled concurrent A/B; do not attribute every timing difference
solely to decoding. Finder resource sampling began after the user initiated paste:
sampled Agent max RSS 41,910,272 bytes, Provider 29,540,352 bytes, at 0.5s intervals.

Evidence: `docs/test-results/performance/native-fixed/`. After verification the test
Agent/peer stopped, test pairing and domain were removed, original extension
registration restored, and both loopback ports were confirmed closed. No persistent
login job was installed. Synthetic output files are retained under ignored test-output.

## Additional typed-IPC candidate (historical state before physical acceptance)

Native sampling showed the provider worker waiting in socket reads in 282/343 stack
samples and the fast decoder in 45/343. The Agent worker still spent many samples
constructing and serializing per-byte `Value` objects. These are sampled stacks,
not exact attribution of total transfer time.

The current Windows source now includes `Node.local_ipc_reply`, serialized directly
by both native IPC adapters. The `local_ipc` Value API remains a compatibility wrapper
around the same validation/read logic. Byte-for-byte equivalence tests cover all
byte values, a maximum-size block, empty payload, both EOF values and large UInt64s.
Protocol and vectors remain unchanged. Swift's sorted-envelope fast path is retained.

With this candidate the diagnostic Mac 64MiB socket hydration took 1.259s
(50.85MiB/s), versus 1.548s for the Finder-tested revision. Windows format/Clippy,
13 Rust tests and release build pass; Mac format/Clippy, 12 Rust tests, Swift checks
and full app build pass. The candidate is staged separately at
`/Users/myhades/Documents/GitHub/rustboard/test-output/performance-probe/typed-repo/target/macos`.
It has NOT been developer-signed or validated in Finder. The earlier signed build
and its evidence are preserved. This distinction must remain explicit until another
native run. No overall physical-device interoperability or near-link-speed acceptance
is claimed by these results.

## Later physical acceptance completed

See [PHYSICAL_INTEROP.md](PHYSICAL_INTEROP.md): the user signed the typed-IPC build,
then a further diagnostic build. Physical Windows Explorer → Mac Finder and Mac
Finder → Windows Explorer both passed 4KiB/64MiB/1GiB destination hashes, as did
bidirectional Unicode clipboard text. This supersedes the candidate's unsigned and
native-untested state above. New data is kept separately under `test-results/interop/`.
Windows → Mac 1GiB fetch took 244.031s; Mac → Windows three-file Explorer paste took
about 225.517s to exclusive-open readiness. Neither meets the near-link-speed target.
