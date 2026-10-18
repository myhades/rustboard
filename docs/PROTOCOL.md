# Rustboard current protocol

The executable schema is `crates/core/src/protocol.rs`; independent vectors are `tests/protocol/vectors.json`. JSON field order is irrelevant. Strings are UTF-8. Integer fields are unsigned, except where explicitly stated; Swift must decode u64 as UInt64, not floating point. Unknown fields are rejected in typed messages.

## Network

TCP protected by TLS1.3 only. ALPN is the ASCII bytes `rustboard/2`. Both endpoints present a device certificate and verify the entire DER certificate's SHA-256 against an explicitly paired fingerprint. TLS CertificateVerify signatures are validated by rustls/ring. There is no public PKI hostname meaning: this is exact certificate pinning. Device ID is the UUID-formatted first16 bytes of the SHA-256 certificate digest (without rewriting UUID version bits). Fingerprints are64 lowercase hexadecimal characters. Both sides must independently pair before connection. Discovery is not trust.

Control frames use a four-byte big-endian unsigned length followed by UTF-8 JSON, bounded to 1 MiB. File Data responses use only the binary framing described in the binary framing section below, bounded to 4 MiB on the network and 64 KiB per local IPC read. No old ALPN, JSON file-data response or capability fallback is supported. The `protocol:1` fields below identify the current metadata/envelope schema; they do not enable a second transport implementation.

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

Every request is authorized against the current peer list, including existing TLS sessions after unpair. File authorization requires the complete `(source_device, offer_id, file_id, version)` tuple; remote paths are never opened. A source serves current or retained file batches under the bounded lifetimes in the retained batch lifetime section below. Clipboard replacement is not transfer revocation.

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

## Binary framing

2026-10-17。仅保留当前实现。Offer 与控制消息的 protocol:1 是当前消息结构标记，不代表支持旧传输。TLS 仍为 1.3、双向证书 pin 与签名验证；无明文或认证降级。

## 网络协商

客户端与服务器只接受 ALPN `rustboard/2`，不提供旧协议协商或回退。
请求及非 Data 响应使用四字节大端长度 + JSON，1 MiB 控制帧上限。

v2 Data 响应格式：

| 字段 | 字节数 | 编码 |
| --- | ---: | --- |
| 长度 | 4 | 大端 u32，最高位必须为 1；其余 31 位为后续长度 |
| request_id | 8 | 大端 u64 |
| offset | 8 | 大端 u64 |
| eof | 1 | 只能是 0 或 1 |
| bytes | 0..4194304 | 原始文件字节 |

接收方在分配前检查长度不超过本次请求及 4 MiB；校验 request_id、offset、文件大小、EOF 一致性。截断、错误类型或超界使连接失效，不透明重试。

## 活动读取窗口

发布、同步元数据、构造 RemoteSource 不读取主体。首次读取只请求调用者所需范围。
随后连续的 64 KiB 读取可请求最多 4 MiB；短小随机读取不启动窗口。
窗口只在已开始的读取期间生效，每个 RemoteSource 最多保留一个窗口；IPC 最多保留两个源连接/窗口。
这属于有界顺序预读，不能再把每个 64 KiB API 请求声称为恰好 64 KiB 网络读取。
取消时最多存在一个已在途的窗口，不预加载整个文件，也不后台持续填充。

源端仍按最多 64 KiB 调用稳定源，每块检查配对、停止标记、保留批次/版本；LocalFile 仍在读前读后检查元数据。
接收端每次交付（包括窗口命中）检查配对、停止/暂停及批次保留期限；读取及批次校验失败丢弃窗口，配对/停止/批次期限检查失败直接拒绝交付。
已在途且已授权的数据不能被远端撤销“收回”，撤销在接收端获知后阻止后续交付。
EOF 从 IPC 缓存移除源；Cancel 清理相应 request_id 的缓存。随机访问不增加并行连接。

## 本地 IPC

身份认证、Unix socket 权限、Windows Named Pipe ACL 不变。文件读取只接受 `read_binary`，最大 64 KiB。
成功 Data 使用上述二进制帧；控制/错误使用 JSON。没有 binary_data 能力字段、旧 read 操作或 JSON 文件数据回退。JSON 成功响应不能代替二进制文件帧。


## Retained file-transfer lifetime

Implementation candidate, 2026-10-18. Windows and Mac automated validation/builds passed; initial user signing and installation completed; small-file physical acceptance has passed as detailed below. The broader concurrency/Handoff release gate remains open.

The current clipboard remains one latest Offer. File batches have a separate bounded registry of metadata and stable source handles. Replacing the clipboard, including with text or unsupported content, retires the presentation without revoking the old batch. Explicit withdrawal/service shutdown remains cancellation. Each read still requires a paired TLS peer, matching source/offer/file/version, valid bounds and a valid stable source. No plaintext transport or previous-format compatibility is added.

A retired batch has a 30-second descriptor-to-read grace period. A validated read renews the whole batch for 120 seconds of idle time, including files not yet requested by a sequential consumer. Active reads can continue past the original Offer presentation TTL. Metadata queries alone do not renew a batch. These are monotonic process-local deadlines; sessions do not survive Agent restart. Retained Offers are not rebroadcast or shown as clipboard history. During the bounded retention window a paired peer with the exact tuple can start another read; this is not a single-use transfer token.

Limits: 8 file batches total, 256 files per batch, aggregate encoded Offer metadata below 512 KiB. No whole-file byte buffering; the existing 4 MiB read window and two-entry IPC connection cache remain. A full registry rejects new file Offers instead of evicting active batches; text remains usable. Completion/cancellation stops requesting bytes, and retired batch handles/clones are reclaimed on idle expiry (not immediately at one file EOF, which could strand other files/readers). Active source replacement or version failure still fails the read. Source handles may keep Windows files locked and APFS clones may retain changed disk blocks until expiry.

Local IPC adds get_file_offer(offer_id) and get_file_offers for retained metadata. read_binary continues to use the existing opaque tuple. Cancellation discards only its request's cached connection, not another concurrent read's cache. Clipboard get_offer still reports only the current item.

Windows IDataObject/IStream retain their original source identities. macOS File Provider resolves an item against its retained batch, not the current clipboard, and checks that batch before/after hydration. Internal batch directories isolate identical names from different Offers; users still paste the original file names. The host's presented-batches.json is a current-format runtime ledger, with no old-ledger import. It evicts only expired batches. Root and working-set changes are signaled after cleanup.
