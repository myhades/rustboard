# Rustboard current protocol

The executable schema is `crates/core/src/protocol.rs`; independent vectors are `tests/protocol/vectors.json`. JSON field order is irrelevant. Strings are UTF-8. Integer fields are unsigned, except where explicitly stated; Swift must decode u64 as UInt64, not floating point. Unknown fields are rejected in typed messages.

## Network

TCP protected by TLS1.3 only. ALPN is the ASCII bytes `rustboard/2`. Both endpoints present a device certificate and verify the entire DER certificate's SHA-256 against an explicitly paired fingerprint. TLS CertificateVerify signatures are validated by rustls/ring. There is no public PKI hostname meaning: this is exact certificate pinning. Device ID is the UUID-formatted first16 bytes of the SHA-256 certificate digest (without rewriting UUID version bits). Fingerprints are64 lowercase hexadecimal characters. Both sides must independently pair before connection. Discovery is not trust.

Control frames use a four-byte big-endian unsigned length followed by UTF-8 JSON, bounded to 1 MiB. File Data responses use only the binary framing described in docs/TRANSPORT_V2.md, bounded to 4 MiB on the network and 64 KiB per local IPC read. No old ALPN, JSON file-data response or capability fallback is supported. The `protocol:1` fields below identify the current metadata/envelope schema; they do not enable a second transport implementation.

## Offer

`{"type":"offer","offer":{...}}` contains:

| Field | Meaning |
|---|---|
| protocol | 1 |
| source_device | UUID derived from authenticated certificate; must match sender |
| offer_id | newly generated UUID |
| event_sequence | Lamport u64; local event increments maximum observed counter |
| expires_unix_ms | Unix milliseconds; greater than now, at most24 hours ahead |
| content | `{"kind":"text","text":"..."}` or `{"kind":"files","files":[...]}` |

Text is at most256 KiB UTF-8 and excludes NUL. A file entry contains `file_id` UUID, `relative_path`, `size` u64 or null, `version` opaque nonempty string ≤128 bytes, and `media_type` string. Max256 entries. Paths use `/` separators; no absolute path, drive, colon/ADS, backslash, control character, dot/parent/empty component, Windows reserved device name, or trailing dot/space. IDs and case-folded paths must be unique. Current Windows presentation rejects directory components and unknown sizes rather than inventing semantics.

Compare Offers lexicographically by `(event_sequence, source_device, offer_id)`; the greater tuple wins. Persist the counter and watermark across restart. Suppress events caused by native publication; never re-originate or forward a received Offer. Expiration does not authorize replay. Device clocks should be reasonably synchronized for the wall-clock expiry check.

## Requests and responses

| type | Fields / response |
|---|---|
| hello | protocol, device_id → matching hello; optional diagnostic exchange |
| poll | no fields → current locally originated offer, otherwise idle |
| offer | offer → ack or error |
| read | request_id, source_device, offer_id, file_id, version, offset, length → data or error |
| data | request_id, offset, bytes, eof; offset and ID must match; EOF must agree with manifest size |
| cancel | request_id → ack |
| error | request_id (u64/null), code |
| ack / idle | no fields |

Error codes: unauthorized, expired, not_found, version_changed, invalid_request, cancelled, busy, io, unsupported. Current implementation uses invalid_request for most invalid file/offer/range lookups and version_changed for source read failure. Do not branch on English diagnostics.

Every request is authorized against the current peer list, including existing TLS sessions after unpair. File authorization requires the complete `(source_device, offer_id, file_id, version)` tuple; remote paths are never opened. A source serves current or retained file batches under the bounded lifetimes in docs/TRANSFER_LIFETIME.md. Clipboard replacement is not transfer revocation.

One request is outstanding per connection. Cancellation is between bounded chunks: stop issuing reads, send Cancel if useful, and close the connection. A synchronous chunk already in progress may finish; Cancel does not interrupt a concurrent syscall. A disconnect never advances the client's cursor. A later request may reconnect with the same tuple and offset, provided the Offer is still current. There is no resume of partially committed destination files without the consumer validating the same version.

## Native local IPC

Control/error JSON and binary file-data framing as above. Windows endpoint is `\\.\pipe\Rustboard-<userSID>-<profile>`, DACL restricted to current user, remote clients rejected. macOS uses an authenticated Unix socket with same-team signed process validation; never expose the local IPC over TCP.

Envelope: `{"protocol":1,"request":{...}}`.

- `{"type":"control","args":["status"]}` etc: native Agent control.
- `{"type":"get_offer"}` → `{"ok":true,"protocol":1,"offer":<Offer or null>}`.
- `{"type":"read_binary","request_id":7,"offer_id":"...","file_id":"...","version":"...","offset":249,"length":5}` → binary Data frame. Source device is resolved from the current authenticated Offer, never supplied as a path.
- `{"type":"get_file_offer","offer_id":"..."}` → retained batch metadata; `get_file_offers` lists retained file batches. Neither operation reads file bodies or renews idle deadlines.
- `{"type":"cancel","request_id":7}` → successful acknowledgment. Stop scheduling further reads; one in-flight bounded request may complete.

Failures: `{"ok":false,"protocol":1,"code":"io","error":"diagnostic"}` in the current Windows adapter. The vocabulary above is reserved; adapters may refine codes compatibly. Consumers must handle all failures without displaying/logging clipboard body or source contents. Control success has `ok:true`; status also includes protocol/device/fingerprint/mode. The raw JSON-vector/native-pipe checker proves offset, version and bounds validation against the live Agent.

File Provider publication/enumeration/signaling stays in the macOS adapter; there is no undocumented Swift network protocol. Use the Rust Node.local_ipc implementation for reads. Native control is a separate adapter responsibility.
