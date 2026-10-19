# Windows native service control — 2026-10-19

Replaced daily CLI registry subprocesses with Win32 registry APIs and PowerShell process polling with ToolHelp enumeration, executable-path matching and process-handle waits. No installation/packaging script changes, new dependency, abstraction layer or release.

## Verification

- Windows: 29 unit tests plus shared protocol vectors passed.
- `cargo clippy --all-targets --locked -- -D warnings`, release build and diff whitespace checks passed.
- Real user PowerShell: seven alternating restart runs per controller, after one warmup each, using the same installed Agent for both. CLI stdout/stderr was captured; successful lifecycle commands remained silent.
- Repeated stop/start passed; stop removed login startup and start restored the identical command. Configuration SHA-256 remained unchanged. Final status reported running.
- Run outside Codex because its MSIX context virtualizes user storage and registry. No file transfer or Mac binary changes were required.

| Round | Before (ms) | Native (ms) |
| --- | ---: | ---: |
| 1 | 4066.38 | 3608.53 |
| 2 | 3658.65 | 3635.49 |
| 3 | 3633.96 | 3615.53 |
| 4 | 3615.78 | 3587.20 |
| 5 | 3632.99 | 3616.21 |
| 6 | 3625.70 | 3606.15 |
| 7 | 3649.32 | 3619.41 |

Median: 3633.96 -> 3615.53 ms (18.43 ms / 0.51% lower). This does not demonstrate a substantial restart latency improvement. Both builds retain the pre-launch status request: when the Agent is stopped, Windows IPC retries connection for three seconds before `start` spawns the Agent. That common wait was identified in source, not separately instrumented in this experiment, and remains outside this subprocess-replacement change.
