# Windows IPC startup wait — 2026-10-19

The Windows IPC client now retries only ERROR_PIPE_BUSY, up to the existing three-second deadline. A missing pipe immediately reports that the Agent is unavailable; other errors retain their native error details. No new abstraction, configuration, installation script changes or release.

Validation before installation: 29 unit tests, shared protocol vectors, clippy with warnings denied and release build passed.

User ran the alternating comparison in normal PowerShell, using the same installed Agent. The baseline already includes native Windows service control (05ab5c8). The user stopped after four rounds; these are the supplied measurements, not a completed seven-round run.

| Round | Before (ms) | After (ms) |
| --- | ---: | ---: |
| 1 | 3592.21 | 623.69 |
| 2 | 3615.43 | 605.48 |
| 3 | 3641.54 | 637.02 |
| 4 | 3569.58 | 556.70 |

Median: 3603.82 -> 614.585 ms, approximately 83% lower. The result is consistent with removing the unnecessary three-second connection retry before spawning the Agent.

Final local verification: stopped status returned in 24.75 ms; repeated stop/start passed, configuration and startup registration were unchanged. Both installed CLI copies matched SHA-256 `3E50207479D856A464BCCCAAA733931F890B261613EF3364088405920CF08B48`; the Scoop entry returned running status.
