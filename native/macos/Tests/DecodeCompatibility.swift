import Foundation

@main struct DecodeCompatibility {
    static func main() throws {
        func packet(_ bytes: String, offset: String = "9007199254740993", request: String = "18446744073709551615") -> Data {
            Data("{\"ok\":true,\"protocol\":1,\"response\":{\"bytes\":[\(bytes)],\"eof\":false,\"offset\":\(offset),\"request_id\":\(request),\"type\":\"data\"}}".utf8)
        }
        for count in [0, 1, 251, 4096, 65536] {
            let bytes = (0..<count).map {UInt8($0 % 251)}
            let data = packet(bytes.map(String.init).joined(separator: ","))
            let fast = try decodeReply(data).response!
            let generic = try JSONDecoder().decode(Reply.self, from: data).response!
            precondition(fast.bytes == bytes && fast.bytes == generic.bytes)
            precondition(fast.request_id == UInt64.max && fast.offset == 9007199254740993)
            precondition(fast.eof == generic.eof && fast.type == generic.type)
            // Every truncation of a small valid packet must still fail.
            if count == 1 {
                for end in 0..<data.count {
                    do {_ = try decodeReply(data.prefix(end)); fatalError("accepted truncated packet")} catch {}
                }
            }
        }
        for data in [packet("256"), packet("-1"), packet("true"), packet("01"),
                     packet("1,"), packet("1,,2"), packet("1", offset: "18446744073709551616"),
                     packet("1", request: "-1"), packet("1") + Data("x".utf8)] {
            do {_ = try decodeReply(data); fatalError("accepted invalid reply: \(String(decoding: data, as: UTF8.self))")} catch {}
        }
        let valid = packet("0,255")
        let pretty = try JSONSerialization.data(withJSONObject: JSONSerialization.jsonObject(with: valid), options: [.prettyPrinted, .sortedKeys])
        let fallback = try decodeReply(pretty)
        precondition(fallback.response!.bytes == [0,255])
        // Unknown Data fields must not get accepted by the optimized path.
        let unknown = Data(String(decoding: valid, as: UTF8.self).replacingOccurrences(of: "\"type\":\"data\"", with: "\"type\":\"data\",\"extra\":1").utf8)
        do {_ = try decodeReply(unknown); fatalError("accepted unknown field")} catch {}
        print("PASS: fast/fallback v1 decoding, 64KiB bytes, UInt64 precision, invalid/truncated frames")
    }
}
