// Diagnostic Unix endpoint from the Core throughput example. Does NOT replace or
// test signed Agent authentication. Runs the production decoder and hydration writer.
import Foundation
import CryptoKit
import Darwin

@main struct SocketHydration {
    static func main() throws {
        let path = CommandLine.arguments[1]
        let destination = URL(fileURLWithPath: CommandLine.arguments[2])
        let fd = socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else {throw WireError.unavailable}
        defer {close(fd)}
        var timeout = timeval(tv_sec: 30, tv_usec: 0)
        setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, socklen_t(MemoryLayout.size(ofValue: timeout)))
        setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, socklen_t(MemoryLayout.size(ofValue: timeout)))
        var noSignal: Int32 = 1; setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &noSignal, 4)
        var address = sockaddr_un(); address.sun_family = sa_family_t(AF_UNIX)
        let pathBytes = Array(path.utf8) + [0]
        guard pathBytes.count <= MemoryLayout.size(ofValue: address.sun_path) else {throw WireError.invalid}
        withUnsafeMutableBytes(of: &address.sun_path) {$0.copyBytes(from: pathBytes)}
        let connected = withUnsafePointer(to: &address) {$0.withMemoryRebound(to: sockaddr.self, capacity: 1) {connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))}}
        guard connected == 0 else {throw WireError.unavailable}
        var callSeconds = 0.0
        func readExact(_ count: Int) throws -> Data {
            var result = Data(count: count)
            try result.withUnsafeMutableBytes { raw in
                var i = 0
                while i < count {let n = read(fd, raw.baseAddress!.advanced(by: i), count-i); guard n > 0 else {throw WireError.unavailable}; i += n}
            }; return result
        }
        func call(_ op: Operation) throws -> Reply {
            let start = ProcessInfo.processInfo.systemUptime
            defer {callSeconds += ProcessInfo.processInfo.systemUptime-start}
            let data = try frame(JSONEncoder().encode(Envelope(request: op)))
            try data.withUnsafeBytes { raw in
                var i = 0
                while i < raw.count {let n = write(fd, raw.baseAddress!.advanced(by: i), raw.count-i); guard n > 0 else {throw WireError.unavailable}; i += n}
            }
            return try readIPCReply(binary: op.type == "read_binary", readExact: readExact)
        }
        guard let offer = try call(Operation(type: "get_offer")).offer, let file = offer.content.files?.first, let size = file.size else {throw WireError.invalid}
        try offer.validate()
        let start = ProcessInfo.processInfo.systemUptime
        try hydrate(offer, file, to: destination, progress: Progress(totalUnitCount: Int64(size)), call: call)
        let seconds = ProcessInfo.processInfo.systemUptime-start
        _ = try call(Operation(type: "cancel", request_id: 1))
        // Hash independently AFTER the timed hydration, without loading the whole file.
        let handle = try FileHandle(forReadingFrom: destination); defer {try? handle.close()}
        var hash = SHA256()
        while let bytes = try autoreleasepool(invoking: {try handle.read(upToCount: 65536)}), !bytes.isEmpty {hash.update(data: bytes)}
        let digest = hash.finalize().map {String(format: "%02x", $0)}.joined()
        let report: [String: Any] = ["mode": "unix_hydrate_binary", "bytes": size,
            "seconds": seconds, "mib_per_second": Double(size)/1048576/seconds,
            "call_seconds": callSeconds, "sha256": digest]
        print(String(data: try JSONSerialization.data(withJSONObject: report, options: .sortedKeys), encoding: .utf8)!)
    }
}
