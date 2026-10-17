// Compile with the production Wire.swift and Security.swift, using swiftc -O.
// Measures JSONDecoder only: no socket, authentication, TLS, File Provider or disk.
import Foundation

@main struct DecodeThroughput {
    static func main() throws {
        let bytes = (0..<65536).map { $0 % 251 }
        let packet = try JSONSerialization.data(withJSONObject: [
            "ok": true, "protocol": 1,
            "response": ["type": "data", "request_id": 1, "offset": 0,
                         "eof": false, "bytes": bytes]
        ], options: .sortedKeys)
        let chunks = 1024
        let start = ProcessInfo.processInfo.systemUptime
        for _ in 0..<chunks {
            try autoreleasepool {
                let reply = try decodeReply(packet)
                guard reply.ok, reply.protocol == 1, let response = reply.response,
                      response.bytes.count == bytes.count, response.bytes[250] == 250,
                      response.bytes[251] == 0 else {throw WireError.invalid}
            }
        }
        let seconds = ProcessInfo.processInfo.systemUptime - start
        let report: [String: Any] = ["mode": "swift_reply_decoder", "chunks": chunks,
            "payload_bytes": chunks * bytes.count, "frame_bytes": packet.count,
            "seconds": seconds, "mib_per_second": 64.0 / seconds,
            "milliseconds_per_chunk": seconds * 1000.0 / Double(chunks)]
        print(String(data: try JSONSerialization.data(withJSONObject: report, options: .sortedKeys), encoding: .utf8)!)
    }
}
