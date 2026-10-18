import Foundation
@main struct BinaryFraming {
    static func main() throws {
        func readPacket(_ data: Data, binary: Bool) throws -> Reply {
            var position = 0
            return try readIPCReply(binary: binary) { count in
                guard count <= data.count-position else {throw WireError.invalid}
                defer {position += count}
                return data.subdata(in:position..<position+count)
            }
        }
        let binary = Data([0x80,0,0,19] + Array(repeating:255,count:8) + [0,32,0,0,0,0,0,1] + [1,0,255])
        let binaryReply = try readPacket(binary,binary:true).response!
        precondition(binaryReply.request_id == UInt64.max && binaryReply.offset == 9007199254740993 && binaryReply.eof && binaryReply.bytes == [0,255])
        for end in 0..<binary.count {
            do {_ = try readPacket(binary.prefix(end),binary:true); fatalError("accepted truncated binary")} catch {}
        }
        do {_ = try readPacket(binary,binary:false); fatalError("unnegotiated binary")} catch {}
        for bad in [Data([255,255,255,255]),Data([0x80,0,0,16])] {
            do {_ = try readPacket(bad,binary:true); fatalError("unbounded binary")} catch {}
        }
        var badEOF = binary; badEOF[20] = 2
        do {_ = try readPacket(badEOF,binary:true); fatalError("invalid EOF")} catch {}
        let jsonData=Data(#"{"ok":true,"protocol":1,"response":{"type":"data","request_id":1,"offset":0,"bytes":[],"eof":true}}"#.utf8)
        do {_ = try readPacket(frame(jsonData),binary:true);fatalError("accepted JSON file data")} catch {}
        let control=Data(#"{"ok":true,"protocol":1,"offer":null}"#.utf8)
        let ok=try readPacket(frame(control),binary:false);precondition(ok.ok)
        do {_ = try readPacket(frame(control),binary:true);fatalError("accepted control success as file data")} catch {}
        let error=Data(#"{"ok":false,"protocol":1,"code":"io"}"#.utf8)
        let failed=try readPacket(frame(error),binary:true);precondition(!failed.ok)
        print("PASS binary-only data, control/error JSON, bounds and truncation")
    }
}
