import Foundation
import CryptoKit
import Darwin

enum WireError: Error { case invalid, unavailable, stale, cancelled }
struct FileEntry: Codable, Equatable {
    let file_id: String
    let relative_path: String
    let size: UInt64?
    let version: String
    let media_type: String
}
struct Content: Codable { let kind: String; let files: [FileEntry]?; let text: String? }
struct Offer: Codable {
    let `protocol`: UInt16
    let source_device: String
    let offer_id: String
    let event_sequence: UInt64
    let expires_unix_ms: UInt64
    let content: Content
    func validate(now: UInt64 = UInt64(Date().timeIntervalSince1970 * 1000), flat: Bool = true) throws {
        guard `protocol` == 1, UUID(uuidString: source_device) != nil, UUID(uuidString: offer_id) != nil,
              expires_unix_ms > now, expires_unix_ms - now <= 86400000 else { throw WireError.stale }
        if content.kind == "text" {
            guard let text = content.text, content.files == nil, text.utf8.count <= 262144, !text.contains("\0") else {throw WireError.invalid}; return
        }
        guard content.kind == "files", content.text == nil, let files = content.files, !files.isEmpty, files.count <= 256 else {throw WireError.invalid}
        var ids = Set<String>(), names = Set<String>()
        for file in files {
            guard UUID(uuidString: file.file_id) != nil, ids.insert(file.file_id.lowercased()).inserted,
                  names.insert(file.relative_path.lowercased()).inserted,
                  !file.version.isEmpty, file.version.utf8.count <= 128, let size = file.size, size <= Int64.max,
                  validPath(file.relative_path), (!flat || !file.relative_path.contains("/")) else {throw WireError.invalid}
        }
    }
}
func validPath(_ path: String) -> Bool {
    guard !path.isEmpty, !path.contains("\\"), !path.contains(":"), !path.unicodeScalars.contains(where: {$0.value < 32 || $0.value == 127}) else {return false}
    for part in path.split(separator: "/", omittingEmptySubsequences: false) {
        guard !part.isEmpty, part != ".", part != "..", !part.hasSuffix("."), !part.hasSuffix(" ") else {return false}
        let base = part.split(separator: ".")[0].uppercased()
        if ["CON","PRN","AUX","NUL","CONIN$","CONOUT$"].contains(base) {return false}
        if (base.hasPrefix("COM") || base.hasPrefix("LPT")) && base.count == 4 && "123456789¹²³".contains(base.last!) {return false}
    }
    return true
}
func itemID(_ offer: Offer, _ file: FileEntry) -> String {
    "rb1:\(offer.source_device):\(offer.offer_id):\(file.file_id):" + SHA256.hash(data: Data(file.version.utf8)).map {String(format: "%02x", $0)}.joined()
}
struct DataMessage: Decodable { let type: String; let request_id: UInt64; let offset: UInt64; let bytes: [UInt8]; let eof: Bool }
struct Reply: Decodable { let ok: Bool; let `protocol`: UInt16; let offer: Offer?; let response: DataMessage? }
// Fast path for the exact JSON emitted by Node.local_ipc's sorted Value map.
// Other valid v1 layouts (field order, whitespace, escapes) use JSONDecoder.
// This is an implementation optimization, not a new encoding or a weaker parser.
func decodeReply(_ packet: Data) throws -> Reply {
    guard !packet.isEmpty, packet.count <= frameLimit else {throw WireError.invalid}
    let fast: Reply? = try packet.withUnsafeBytes { raw -> Reply? in
        let input = raw.bindMemory(to: UInt8.self)
        var i = 0
        func whitespace() {
            while i < input.count, input[i] == 32 || input[i] == 9 || input[i] == 10 || input[i] == 13 {i += 1}
        }
        func take(_ literal: StaticString) -> Bool {
            literal.withUTF8Buffer { expected in
                guard expected.count <= input.count - i else {return false}
                for c in expected {guard input[i] == c else {return false}; i += 1}
                return true
            }
        }
        func number() -> UInt64? {
            guard i < input.count, input[i] >= 48, input[i] <= 57 else {return nil}
            let first = i
            var value: UInt64 = 0
            while i < input.count, input[i] >= 48, input[i] <= 57 {
                let digit = UInt64(input[i] - 48)
                guard value <= (UInt64.max - digit) / 10 else {return nil}
                value = value * 10 + digit; i += 1
            }
            guard i - first == 1 || input[first] != 48 else {return nil}
            return value
        }
        guard take("{\"ok\":true,\"protocol\":1,\"response\":{\"bytes\":[") else {return nil}
        var bytes = [UInt8](); bytes.reserveCapacity(65536)
        whitespace()
        if i < input.count, input[i] != 93 {
            while true {
                whitespace()
                guard bytes.count < 65536, let n = number(), n <= 255 else {throw WireError.invalid}
                bytes.append(UInt8(n))
                whitespace()
                guard i < input.count else {throw WireError.invalid}
                if input[i] != 44 {break}
                i += 1
            }
        }
        guard i < input.count, input[i] == 93 else {throw WireError.invalid}
        guard take("],\"eof\":") else {return nil}
        let eof: Bool
        if i < input.count, input[i] == 116 {guard take("true") else {return nil}; eof = true}
        else {guard take("false") else {return nil}; eof = false}
        guard take(",\"offset\":"), let offset = number(),
              take(",\"request_id\":"), let requestID = number(),
              take(",\"type\":\"data\"}}"), i == input.count else {return nil}
        return Reply(ok: true, protocol: 1, offer: nil,
            response: DataMessage(type: "data", request_id: requestID, offset: offset, bytes: bytes, eof: eof))
    }
    if let fast {return fast}
    return try JSONDecoder().decode(Reply.self, from: packet)
}
struct Operation: Encodable {
    let type: String
    var request_id: UInt64? = nil
    var offer_id: String? = nil
    var file_id: String? = nil
    var version: String? = nil
    var offset: UInt64? = nil
    var length: UInt32? = nil
}
struct Envelope: Encodable { let `protocol`: UInt16 = 1; let request: Operation }
let frameLimit = 1048576
func frame(_ bytes: Data) throws -> Data {
    guard !bytes.isEmpty, bytes.count <= frameLimit else {throw WireError.invalid}
    let n = UInt32(bytes.count)
    return Data([UInt8(truncatingIfNeeded: n >> 24), UInt8(truncatingIfNeeded: n >> 16), UInt8(truncatingIfNeeded: n >> 8), UInt8(truncatingIfNeeded: n)]) + bytes
}
func frameLength(_ bytes: Data) throws -> Int {
    guard bytes.count == 4 else {throw WireError.invalid}
    let n = bytes.reduce(UInt32(0)) {($0 << 8) | UInt32($1)}
    guard n > 0, n <= frameLimit else {throw WireError.invalid}; return Int(n)
}
func ipc(_ operation: Operation) throws -> Reply {try ipc(operation, profile: ProcessInfo.processInfo.environment["RUSTBOARD_PROFILE"] ?? "default")}
func ipc(_ operation: Operation, profile: String) throws -> Reply {
    try IPCSession(profile: profile).call(operation)
}
// A fetch owns one authenticated socket, with one v1 request outstanding at a time.
// No reconnect/retry can hide Agent restart or commit a partially hydrated file.
final class IPCSession {
    private let fd: Int32
    init(profile: String) throws {
    guard !profile.isEmpty, profile.count <= 32, profile.utf8.allSatisfy({(48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0) || $0 == 45}) else {throw WireError.invalid}
    let path = try groupDirectory().appendingPathComponent(profile).appendingPathComponent("agent.sock").path
    fd = socket(AF_UNIX, SOCK_STREAM, 0)
    guard fd >= 0 else {throw WireError.unavailable}
    var ready = false
    defer {if !ready {close(fd)}}
    var timeout = timeval(tv_sec: 10, tv_usec: 0)
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout.size(ofValue: timeout)))
    setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, socklen_t(MemoryLayout.size(ofValue: timeout)))
    var yes: Int32 = 1; setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &yes, 4)
    var address = sockaddr_un(); address.sun_family = sa_family_t(AF_UNIX)
    let bytes = Array(path.utf8) + [0]
    guard bytes.count <= MemoryLayout.size(ofValue: address.sun_path) else {throw WireError.invalid}
    withUnsafeMutableBytes(of: &address.sun_path) { $0.copyBytes(from: bytes) }
    let connected = withUnsafePointer(to: &address) { p in p.withMemoryRebound(to: sockaddr.self, capacity: 1) {connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))} }
    guard connected == 0 else {throw SecurityFailure.stage("socket_connect",errno)}
    try authenticateSocket(fd, server: true)
    ready = true
    }
    deinit {close(fd)}
    func call(_ operation: Operation) throws -> Reply {
    let packet = try frame(JSONEncoder().encode(Envelope(request: operation)))
    try packet.withUnsafeBytes { raw in
        var offset = 0
        while offset < raw.count {let n = write(fd, raw.baseAddress!.advanced(by: offset), raw.count-offset); guard n > 0 else {throw WireError.unavailable}; offset += n}
    }
    func readExact(_ count: Int) throws -> Data {
        var result = Data(count: count)
        try result.withUnsafeMutableBytes { raw in
            var offset = 0
            while offset < count {let n = read(fd, raw.baseAddress!.advanced(by: offset), count-offset); guard n > 0 else {throw WireError.unavailable}; offset += n}
        }; return result
    }
    let reply = try decodeReply(readExact(frameLength(readExact(4))))
    guard reply.ok, reply.protocol == 1 else {throw WireError.unavailable}; return reply
    }
}

func currentOffer(profile: String = ProcessInfo.processInfo.environment["RUSTBOARD_PROFILE"] ?? "default") throws -> Offer? {
    let offer = try ipc(Operation(type: "get_offer"), profile: profile).offer
    try offer?.validate(); return offer
}
// Injectable transport makes faults and cancellation testable without a second protocol.
func hydrate(_ offer: Offer, _ file: FileEntry, to url: URL, progress: Progress,
             call: (Operation) throws -> Reply = ipc,
             commit: (URL) throws -> Void = {_ in}) throws {
    guard let size = file.size, size <= Int64.max else {throw WireError.invalid}
    let fd = open(url.path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, S_IRUSR | S_IWUSR)
    guard fd >= 0 else {throw WireError.unavailable}
    var success = false
    defer {close(fd); if !success {unlink(url.path)} }
    guard flock(fd, LOCK_EX | LOCK_NB) == 0 else {throw WireError.unavailable}
    let requestID = UInt64.random(in: 1...UInt64.max)
    func checkCurrent() throws {
        guard !progress.isCancelled else {throw WireError.cancelled}
        let reply = try call(Operation(type: "get_offer"))
        guard reply.ok, reply.protocol == 1, let current = reply.offer, current.source_device == offer.source_device, current.offer_id == offer.offer_id,
              current.expires_unix_ms > UInt64(Date().timeIntervalSince1970 * 1000),
              current.content.files?.contains(file) == true else {throw WireError.stale}
    }
    do {
        try checkCurrent()
        var offset: UInt64 = 0
        // Empty files still make a read to validate source identity and EOF.
        repeat {
            guard !progress.isCancelled else {throw WireError.cancelled}
            let length = UInt32(min(65536, max(1, size-offset)))
            let reply = try autoreleasepool { try call(Operation(type: "read", request_id: requestID, offer_id: offer.offer_id, file_id: file.file_id, version: file.version, offset: offset, length: length)) }
            guard reply.ok, reply.protocol == 1, let data = reply.response,
                  data.type == "data", data.request_id == requestID, data.offset == offset,
                  data.bytes.count <= Int(length), UInt64(data.bytes.count) <= size-offset,
                  data.eof == (offset + UInt64(data.bytes.count) == size),
                  !data.bytes.isEmpty || offset == size else {throw WireError.invalid}
            guard !progress.isCancelled else {throw WireError.cancelled}
            try data.bytes.withUnsafeBytes { raw in
                var done = 0
                while done < raw.count {let n = write(fd, raw.baseAddress!.advanced(by: done), raw.count-done); guard n > 0 else {throw WireError.unavailable}; done += n}
            }
            offset += UInt64(data.bytes.count); progress.completedUnitCount = Int64(offset)
            guard !progress.isCancelled else {throw WireError.cancelled}
        } while offset < size
        try checkCurrent()
        guard fsync(fd) == 0, !progress.isCancelled else {throw WireError.cancelled}
        try commit(url)
        success = true
    } catch {
        _ = try? call(Operation(type: "cancel", request_id: requestID)); throw error
    }
}

private struct AnyKey: CodingKey {
    var stringValue: String
    init?(stringValue:String) {self.stringValue=stringValue}
    var intValue:Int? {nil}
    init?(intValue:Int) {return nil}
}
private func checkKeys(_ decoder: Decoder, _ allowed: Set<String>) throws {
    let keys=try decoder.container(keyedBy:AnyKey.self).allKeys.map(\.stringValue)
    guard Set(keys).isSubset(of:allowed) else {throw WireError.invalid}
}
extension FileEntry {
    init(from decoder:Decoder) throws {
        try checkKeys(decoder,["file_id","relative_path","size","version","media_type"])
        let c=try decoder.container(keyedBy:CodingKeys.self)
        file_id=try c.decode(String.self,forKey:.file_id);relative_path=try c.decode(String.self,forKey:.relative_path)
        size=try c.decodeIfPresent(UInt64.self,forKey:.size);version=try c.decode(String.self,forKey:.version);media_type=try c.decode(String.self,forKey:.media_type)
    }
}
extension Content {
    init(from decoder:Decoder) throws {
        let c=try decoder.container(keyedBy:CodingKeys.self);kind=try c.decode(String.self,forKey:.kind)
        guard ["text","files"].contains(kind) else {throw WireError.invalid}
        try checkKeys(decoder,kind == "text" ? ["kind","text"]:["kind","files"])
        files=try c.decodeIfPresent([FileEntry].self,forKey:.files);text=try c.decodeIfPresent(String.self,forKey:.text)
    }
}
extension Offer {
    init(from decoder:Decoder) throws {
        try checkKeys(decoder,["protocol","source_device","offer_id","event_sequence","expires_unix_ms","content"])
        let c=try decoder.container(keyedBy:CodingKeys.self)
        `protocol`=try c.decode(UInt16.self,forKey:.protocol);source_device=try c.decode(String.self,forKey:.source_device)
        offer_id=try c.decode(String.self,forKey:.offer_id);event_sequence=try c.decode(UInt64.self,forKey:.event_sequence)
        expires_unix_ms=try c.decode(UInt64.self,forKey:.expires_unix_ms);content=try c.decode(Content.self,forKey:.content)
    }
}
extension DataMessage {
    init(from decoder:Decoder) throws {
        try checkKeys(decoder,["type","request_id","offset","bytes","eof"])
        let c=try decoder.container(keyedBy:CodingKeys.self)
        type=try c.decode(String.self,forKey:.type);request_id=try c.decode(UInt64.self,forKey:.request_id)
        offset=try c.decode(UInt64.self,forKey:.offset);bytes=try c.decode([UInt8].self,forKey:.bytes);eof=try c.decode(Bool.self,forKey:.eof)
    }
    private enum CodingKeys:String,CodingKey {case type,request_id,offset,bytes,eof}
}

// Private metadata bookkeeping, unrelated to the shared wire protocol. A change
// anchor acknowledges a generation only after File Provider applies that batch.
private struct RetiredIndex: Codable {
    var generation: UInt64 = 0
    var entries: [String: UInt64] = [:]
}
func retiredItems(profile: String, adding: [String] = [], acknowledged: UInt64? = nil) throws -> ([String], UInt64) {
    let directory = try groupDirectory().appendingPathComponent(profile)
    let lockFD = open(directory.appendingPathComponent("metadata.lock").path, O_RDWR | O_CREAT | O_NOFOLLOW, 0o600)
    guard lockFD >= 0 else {throw WireError.unavailable}
    defer {close(lockFD)}
    guard flock(lockFD, LOCK_EX) == 0 else {throw WireError.unavailable}
    defer {flock(lockFD, LOCK_UN)}
    let url = directory.appendingPathComponent("retired.json")
    var index = RetiredIndex()
    if FileManager.default.fileExists(atPath: url.path) {
        let attrs = try FileManager.default.attributesOfItem(atPath: url.path)
        guard let size = attrs[.size] as? NSNumber, size.intValue <= frameLimit else {throw WireError.invalid}
        let data = try Data(contentsOf: url)
        if let current = try? JSONDecoder().decode(RetiredIndex.self, from: data) {index = current}
        else {
            let legacy = try JSONDecoder().decode([String].self, from: data)
            index.generation = 1
            for id in legacy {index.entries[id] = 1}
        }
    }
    if let acknowledged, acknowledged <= index.generation {
        index.entries = index.entries.filter {$0.value > acknowledged}
    }
    if !adding.isEmpty {
        guard index.generation < UInt64.max else {throw WireError.invalid}
        index.generation += 1
        for id in adding {index.entries[id] = index.generation}
    }
    guard index.entries.count <= 4096 else {throw WireError.unavailable}
    try JSONEncoder().encode(index).write(to: url, options: .atomic)
    guard chmod(url.path, 0o600) == 0 else {throw WireError.unavailable}
    return (index.entries.keys.sorted(), index.generation)
}

// Never unlink an active hydration owned by another extension instance. Completed
// handoff files have a different prefix and belong to the File Provider framework.
func cleanupPending(in directory: URL) throws -> Int {
    var removed = 0
    for url in try FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil) where url.lastPathComponent.hasPrefix("rb-pending-") {
        let fd = open(url.path, O_RDWR | O_NOFOLLOW)
        if fd < 0 {continue}
        if flock(fd, LOCK_EX | LOCK_NB) == 0 {
            if unlink(url.path) == 0 {removed += 1}
        }
        close(fd)
    }
    return removed
}
