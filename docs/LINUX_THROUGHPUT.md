# Kali ↔ Windows throughput — 2026-10-19

Follow-up: [Core investigation](CORE_THROUGHPUT.md) separates fixture CPU overhead from transport.
The original measurements below are retained as recorded.

Build: `25b4138`, release profile. Windows `192.168.100.6` ↔ Kali VM
`192.168.100.156`, direct LAN TCP; no SSH tunnel. Windows Ethernet reports 1 Gbps.
Kali → Windows RTT averaged 1.15 ms over ten probes, with no packet loss.

## Results

Each Rustboard run transfers a 1 GiB generated pattern. Three runs per direction/path.
Rates below are decimal Mbps; disk paths include destination flush. All six destination
SHA-256 values match `9cc5601236c455c6af19a76e64d2d95953a93b10eeb8b8b756a57090e1499b3e`.

| Measurement | Kali → Windows | Windows → Kali |
|---|---:|---:|
| iperf single stream, initial | 930.7 Mbps | 946.3 Mbps |
| iperf four streams | 936.4 Mbps | 949.3 Mbps |
| iperf single stream, final | 936.6 Mbps | 938.0 Mbps |
| Core TLS + inline pattern validation, median | 712.7 Mbps | 770.6 Mbps |
| Native file API → disk, median | 769.1 Mbps | 696.8 Mbps |
| Native file API → disk, range | 766.8–769.2 Mbps | 668.0–707.1 Mbps |
| Native file API → disk, median time | 11.17 s | 12.33 s |
| Receiver peak RSS | 20.2 MiB | 21.9 MiB |
| Receiver CPU, one-core equivalent | 20–25% | 43–46% |

Windows reception uses the production IDataObject/IStream implementation in an in-process
COM diagnostic, writing a real destination file. Linux reception uses the production Agent
and its FUSE mount in an isolated X11 session, read into a real destination file. These are
platform file API measurements, not Explorer/Thunar GUI timing. Linux CPU/RSS cover the
Agent; Windows CPU/RSS cover the COM diagnostic process. The Linux copy driver is separate.
CPU counters and RSS are sampled; RSS does not include the OS filesystem cache.

Core measurements validate every byte inside the timed loop. Their totals therefore cannot
be compared directly with disk measurements, whose hashes are calculated afterward. Time
spent inside Core read calls was approximately 10.4 s for Kali → Windows and 10.2 s for
Windows → Kali; excluding validation time is not a separate continuous-stream benchmark.

Source data is generated on demand, not read from a source disk. Local same-volume copies
with a warm source and destination flush took 1.18–1.58 s on Windows and 1.34–2.58 s on Kali.
The source generation and both local disk checks are separate from normal clipboard capture.

## Findings

The link sustains near-gigabit TCP in both directions. Rustboard's measured destination
throughput reaches about 82% of that baseline toward Windows and 74% toward Kali. Neither
receiver exhausts one CPU core or grows memory with the 1 GiB file. Local storage has more
throughput than this network, although destination flush remains part of elapsed copy time.

The remaining gap needs profiling of TLS framing/buffer copies, request scheduling and the
FUSE read path. Current transfers use a 4 MiB read window. These measurements do not isolate
one of those components as the cause, and do not justify adding parallel connections yet.

## Reproduction and evidence

`lan_throughput init DIR` creates disposable diagnostic identities.
`serve DIR ADDRESS PEER_PIN 1073741824` serves the generated source.
`receive DIR ADDRESS PEER_PIN` measures Core with inline validation.
On Windows, append `com NEW_OUTPUT_FILE` to measure COM-to-disk; verify its hash separately.
The diagnostic never accesses the system clipboard.

Raw measurement summaries are in [test-results/linux-throughput](test-results/linux-throughput/).
The full run scripts, process measurements and logs are in
`test-output/archive-20261019/throughput-20261019/` on both machines. Test listeners and Agents were stopped;
1 GiB destination files were removed after hashing. The temporary Kali profile's keyring
entry was removed. Existing installed identities, pairing and services were not changed.
