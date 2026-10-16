# Architecture

## Responsibility boundaries

`rustboard-core`: serde protocol v1, bounded frames, Offer validation and deterministic arbitration, handle-backed and deterministic ReadAt sources, TLS 1.3 transport, peer authorization, portable File Provider IPC operations. No Win32, COM, Swift, AppKit or FUSE types.

`rustboard-platform`: `ClipboardBackend` (text publishing) and `FilePresentationBackend` (native file exposure). `PresentedFile` holds a portable `Arc<dyn ReadAt>`. Native event delivery is intentionally platform-specific, rather than hidden behind a universal polling abstraction.

`rustboard-platform/src/windows`: hidden top-level window and WM_CLIPBOARDUPDATE; Unicode text and CF_HDROP capture; read-sharing-only file handles; IDataObject and IStream; per-user named pipe; configuration ACL. WindowsBackend implements both publishing traits on the OLE STA.

`rustboardd`: GUI-subsystem executable with no visible window/tray. Starts a user-session agent, not a Session 0 service. `rustboard`: console executable calling the existing Agent over IPC. `version`/help are local and do not need an Agent.

## Threading and ownership

The main thread initializes OLE, owns the hidden window and IDataObject, and pumps Windows messages. COM-generated interface reference counting owns each data object/stream. A stream has its own cursor mutex and strong source reference; cloning duplicates the cursor but shares the source. STGMEDIUM ownership passes to the caller; streams are released by ReleaseStgMedium. No OleFlushClipboard call is made.

A listener thread accepts at most eight active TLS sessions; each session uses bounded synchronous reads on an independent worker. A single polling worker contacts up to sixteen configured peers. One IPC thread handles a single request per connection. The native incoming queue is a single replaceable latest Offer. These deliberately simple blocking workers avoid a Tokio/STA dependency cycle.

IStream reads run in the OLE apartment and may block on a remote source. Socket operations time out after five seconds, connect after three; the stream checks a thirty-second callback deadline between chunks. Consequently a slow source can delay the STA temporarily. There is no async OLE operation implementation or free-threaded marshaler yet. This is a known responsiveness limitation, not a claim of nonblocking COM.

## Data flow

Local clipboard event → native capture → portable Engine.local → metadata-only TLS Offer. The source registry maps opaque file IDs to already opened handles. Absolute paths are never sent.

Authenticated remote Offer → validate IDs/path/lifetime → Lamport comparison → latest native queue → text clipboard or virtual-file data object. Requesting descriptors only allocates bounded metadata. Explorer requests lindex N, obtains a fresh IStream, then Read/Seek invokes RemoteSource. Reads use a separate TLS connection from offer polling and retain that connection for subsequent chunks. Each source call is at most64 KiB; JSON Data payloads are bounded by the1 MiB frame cap.

No content cache is currently used. Repeated paste rereads the source. This trades bandwidth for simple invalidation and avoids hidden disk usage. Transient connection failure clears the cached connection; a later read can reconnect, but a failed Explorer operation is not automatically restarted.

## Lifetimes

Default Offer lifetime is10 minutes; accepted protocol lifetimes cannot exceed24 hours. Engine expires the current source registry; streams also check remote expiry/trust/pause. Pausing withdraws current sources. Clipboard replacement invalidates the previous Offer. Existing COM references remain valid objects but can return a read failure after revocation/expiry. Agent exit disconnects out-of-process COM consumers.

Clock and conflict watermark persist before publication. Local clipboard contents and source paths do not persist. Identity and explicitly paired peers persist. Runtime pause/mode do not persist across restart in this version.

## Integration ownership

Shared protocol changes belong to this main repository and require vector changes before native consumers adopt them. macOS owns AppKit, Swift File Provider and the minimal host app under native/macos; Linux owns X11/Wayland capability detection and FUSE. Unsupported native platforms currently fail with a clear error rather than pretend to provide a working backend. The shared crates and CLI version path remain buildable.
