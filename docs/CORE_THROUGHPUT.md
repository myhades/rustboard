# Core throughput investigation — 2026-10-19

Follow-up to [the initial LAN measurements](LINUX_THROUGHPUT.md). Same direct Kali ↔ Windows
LAN, 1 GiB per run, three runs per group/direction, release builds. No COM, FUSE, clipboard
or destination disk is involved in this round. Every received byte is checked.

## Measurement overhead

The original fixture generated each byte using integer modulo, and the receiver repeated
that calculation inside the timed loop. The revised fixture precomputes a 65,786-byte
pattern and uses bounded copies; the receiver compares against the same pattern in bulk.
It does not allocate the entire source file. Production transport was unchanged for this
comparison. Removing fixture overhead moved the memory-source result from 707–715 to
872–883 Mbps toward Windows, and from 770–772 to 870–883 Mbps toward Kali.

## Real file sources and buffer reuse

The file-source runs use the shared `LocalFile` implementation and warm source files,
including its before/after metadata checks. Receiving still performs full content validation.

| Core path | Kali → Windows | Windows → Kali |
|---|---:|---:|
| Memory source, unchanged core | 872–883 Mbps | 870–883 Mbps |
| File source, unchanged core | 872–875 Mbps | 816–829 Mbps |
| File source, reused buffers | 882–884 Mbps | 823–840 Mbps |
| Receiver peak RSS, before → after | 19.6 → 15.6 MiB | 16.0 → 8.1 MiB |

The small throughput differences are not enough to claim a substantial speed improvement.
The repeatable benefit is lower allocation pressure and peak receiver memory. The core now
reuses its send/receive Vec allocations between responses. The 4 MiB window, framing,
authentication, source validation and request ordering remain the same. Completed small reads
clear the cached length while retaining capacity, so repeated reads retain their existing
source-validation behavior.

## Phase timing

Temporary diagnostic instrumentation measured source lookup, allocation and reads together
as response preparation, then the TLS write separately. It is not retained in production.

- Kali memory source: 0.112 s preparing 257 responses totaling 1 GiB; 5.025 s inside TLS writes.
- Windows memory source: 0.319 s preparation; 1.828 s inside TLS writes.
- Windows shared file source: 0.810 s preparation; 1.673 s inside TLS writes.

The roughly 0.49 s additional preparation with the Windows file source explains much of its
memory/file difference. The shared file helper performs a seek/read and metadata checks for
each 64 KiB chunk. The follow-up below separates those syscalls individually.

TLS write time includes encryption, buffering and socket blocking; it is not encryption CPU
alone. Socket buffering means write completion is not delivery completion. The remaining
end-to-end time includes transmission still in flight, receiver decryption/validation and
waiting for the next sequential request. The follow-up below isolates these costs before changing the sender.

## Validation

Windows: 33 unit tests plus shared protocol vectors; Linux: 24 unit tests plus vectors.
Both pass all-target clippy. Existing tests cover truncation, frame bounds, source mutation,
revocation, lazy reads, active batches and idle reconnection. The framing test additionally
checks reuse of the provided allocation. All 18 full-size runs in the results file validated
all received bytes.

Results: [core-throughput/results.json](test-results/core-throughput/results.json).
Phase totals: [core-throughput/phases.json](test-results/core-throughput/phases.json).
Full scripts/logs remain in `test-output/archive-20261019/core-perf/` and `test-output/archive-20261019/throughput-20261019/`.
The installed applications were not replaced.


## TLS, window and file-read breakdown (follow-up)

Diagnostic copies of rustls and the transport were instrumented with thread-local timers.
The patch and stage logging are confined to ignored test directories; the application has
no profiling branch or patched dependency. All cases transfer and validate 1 GiB. The
receiver consumes bytes in memory, so these are core results, not desktop paste or disk-write
measurements. File sources are warm, regular local files.

Baseline wall time per GiB, seconds:

| Measured operation | Kali → Windows | Windows → Kali |
|---|---:|---:|
| Sender TLS plaintext write/record construction | 4.729 | 0.520 |
| Receiver TLS packet processing | 0.465 | 4.698 |
| Sender metadata checks | 0.036 | 0.206 |
| Sender seek | 0.010 | 0.008 |
| Sender file read | 0.104 | 0.396 |
| Receiver waiting for response header | 0.284 | 0.769 |
| Receiver work between windows | 0.136 | 0.085 |
| Total throughput | 879 Mbps | 842 Mbps |

These are overlapping wall-time scopes, not additive CPU costs. TLS processing includes
record/buffer handling; socket-read time includes network waiting. Header wait overlaps
TLS and socket reads. Detailed counters, including memory-source controls, are in
[stages.json](test-results/core-throughput/stages.json).

The Kali VM exposes AES but not PCLMULQDQ. In this build, ring cannot use its complete
hardware AES-GCM path. A diagnostic ChaCha20 run reduced Kali TLS record processing from
about 4.7 to 0.8 seconds per GiB, while file throughput only changed to 884/866 Mbps.
This chiefly identifies a CPU cost, rather than the entire throughput gap.

The original sender prepared a whole 4 MiB response before starting to send it. It now
writes the same frame header and streams checked 64 KiB reads into TLS. File checks can
therefore overlap network delivery. With the original AES preference, the diagnostic
file runs reached 899/900 Mbps, and total response-header wait fell to 0.101/0.141 seconds.
The sender uses a reusable 64 KiB buffer; the receiver retains its bounded 4 MiB window.
There are no extra connections, worker threads or protocol changes. A failure after the
header closes the stream; the receiver rejects the incomplete response before returning
any of that response to its caller.

The candidate also prefers rustls's existing ChaCha20 suite on x86 CPUs lacking AES or
PCLMULQDQ. A server with that preference uses its order; other servers honor the client's
order. Other architectures retain the existing provider preference. All existing TLS 1.3
suites, certificate authentication and pin checks remain in place.

Windows passed 34 unit tests and Linux passed 25, both with protocol vectors and all-target
clippy. A new regression covers source failure after one chunk of a multi-chunk response:
no partial bytes reach the caller. Existing tests cover source mutation, transfer locks,
lazy reads, revocation and clipboard replacement.


### Candidate repetition and limits

The first clean candidate batch delivered Kali → Windows at 870–892 Mbps (median 872),
with Windows → Kali completing one run at 879 Mbps. Both directions negotiated
TLS13_CHACHA20_POLY1305_SHA256. Every completed transfer validated all bytes.
Kali sender CPU totaled 3.31 seconds across three GiB, compared with 4.99 seconds for one
GiB in the AES streaming diagnostic. Kali receiver CPU was 1.40 vs 5.24 seconds per GiB.
Windows receiver CPU in the candidate runs was 2.92–3.30 seconds per GiB. Peak RSS was
4.3 MiB for the Kali sender, 8.2 MiB for the Kali receiver, and 15.4 MiB for the Windows
receiver. The Windows sender peaked at 13 MiB; that process includes an interrupted run.
CPU is sampled process CPU time, not a percentage of all host cores.

Subsequent measurements became unstable. Candidate receive speed fell to 821–859 Mbps;
Windows → Kali timed out with EAGAIN. Adjacent confirmation runs using both the AES
streaming version and the original buffered sender also timed out in that direction.
Contemporaneous single-stream iperf measured 602 Mbps forward and 924 Mbps reverse,
versus roughly 937–949 Mbps earlier. These observations do not establish the cause of
the pauses, but they do not isolate it to either new change. Timeouts were not increased
or masked. The failed transfers are retained in the results alongside successful ones.

The 899/900 Mbps streaming diagnostic is approximately 96% of the earlier measured link
ceiling. The final low-CPU combination has completed both directions, but a stable
three-run-per-direction acceptance is still outstanding because of the later pauses.
No claim is made that desktop pastes now sustain that speed. The installed applications
and their configuration were not changed.

Candidate samples and failures: [candidate.json](test-results/core-throughput/candidate.json).
