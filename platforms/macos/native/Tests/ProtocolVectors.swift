import Foundation
import CryptoKit

@main struct ProtocolVectors {
    static func main() throws {
        let vector = try Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1]))
        struct Vectors: Decodable {let offer: Offer; let data: DataMessage; let invalid_paths: [String]}
        let v = try JSONDecoder().decode(Vectors.self, from: vector)
        try v.offer.validate(now: 999999, flat: false)
        precondition(v.offer.content.files![0].relative_path == "folder/样例.bin")
        precondition(v.data.offset == 249 && v.data.bytes == [249,250,0,1,2])
        precondition(v.invalid_paths.allSatisfy {!validPath($0)})
        let badData=Data(#"{"type":"data","request_id":7,"offset":249,"bytes":[256],"eof":false}"#.utf8)
        do {_ = try JSONDecoder().decode(DataMessage.self,from:badData);fatalError("accepted invalid byte")} catch {}
        let unknown=Data(#"{"type":"data","request_id":7,"offset":249,"bytes":[1],"eof":false,"extra":true}"#.utf8)
        do {_ = try JSONDecoder().decode(DataMessage.self,from:unknown);fatalError("accepted unknown field")} catch {}
        let encoded = try JSONEncoder().encode(Envelope(request: Operation(type:"read_binary",request_id:UInt64.max,offer_id:v.offer.offer_id,file_id:v.offer.content.files![0].file_id,version:"pattern-v1",offset:9007199254740993,length:5)))
        let string=String(data:encoded,encoding:.utf8)!
        precondition(string.contains("18446744073709551615") && string.contains("9007199254740993"))
        let framedLength = try frameLength(frame(encoded).prefix(4)); precondition(framedLength == encoded.count)
        for bad in [Data([0,0,0,0]),Data([255,255,255,255]),Data([0,0,0])] {
            do {_ = try frameLength(bad);fatalError("accepted invalid frame")} catch {}
        }
        let file=FileEntry(file_id:v.offer.content.files![0].file_id,relative_path:"sample.bin",size:131073,version:"pattern-v1",media_type:"application/octet-stream")
        let offer=Offer(protocol:1,source_device:v.offer.source_device,offer_id:v.offer.offer_id,event_sequence:UInt64.max-2,expires_unix_ms:UInt64(Date().timeIntervalSince1970*1000)+600000,content:Content(kind:"files",files:[file],text:nil))
        try offer.validate()
        let roundtrip=try JSONDecoder().decode(Offer.self,from:JSONEncoder().encode(offer))
        precondition(roundtrip.event_sequence==UInt64.max-2)
        let rawMax=Data(#"{"type":"data","request_id":18446744073709551615,"offset":9007199254740993,"bytes":[],"eof":true}"#.utf8)
        let maxMessage=try JSONDecoder().decode(DataMessage.self,from:rawMax)
        precondition(maxMessage.request_id==UInt64.max && maxMessage.offset==9007199254740993)
        precondition(itemID(offer,file).hasPrefix("rb1:"+offer.source_device+":"+offer.offer_id+":"+file.file_id+":"))
        let folder=FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        try FileManager.default.createDirectory(at:folder,withIntermediateDirectories:false)
        defer {try? FileManager.default.removeItem(at:folder)}
        for mode in ["success","cancel","disconnect","offset","request_id","early_eof","late_eof","short","version","restart","commit_failure"] {
            var reads=0;var cancels=0
            let progress=Progress(totalUnitCount:Int64(file.size!))
            let url=folder.appendingPathComponent(mode)
            func call(_ op:Operation) throws -> Reply {
                if op.type == "cancel" {cancels += 1;return Reply(ok:true,protocol:1,offer:nil,response:nil)}
                if op.type == "get_offer" {fatalError("hydration must not consult the current clipboard")}
                if op.type == "get_file_offer" {
                    precondition(op.offer_id == offer.offer_id)
                    return Reply(ok:true,protocol:1,offer:(mode == "restart" && reads > 0) ? nil : offer,response:nil)
                }
                reads += 1
                if mode == "disconnect" && reads == 2 {throw WireError.unavailable}
                if mode == "version" && reads == 2 {return Reply(ok:false,protocol:1,offer:nil,response:nil)}
                if mode == "cancel" && reads == 2 {progress.cancel()}
                let start=op.offset!, size=min(UInt64(op.length!),file.size!-start)
                let bytes=(0..<size).map {UInt8((start+$0)%251)}
                let message=DataMessage(type:"data",request_id:op.request_id!+(mode == "request_id" ? 1:0),offset:start+(mode == "offset" ? 1:0),bytes:mode == "short" ? []:bytes,eof:mode == "early_eof" ? true:(mode == "late_eof" ? false:start+size==file.size!))
                return Reply(ok:true,protocol:1,offer:nil,response:message)
            }
            do {
                try hydrate(offer,file,to:url,progress:progress,call:call,commit: {_ in if mode == "commit_failure" {throw WireError.unavailable}})
                precondition(mode == "success")
                let bytes=try Data(contentsOf:url)
                precondition(bytes.count==131073 && bytes.enumerated().allSatisfy {$0.element==UInt8($0.offset%251)})
                let attrs=try FileManager.default.attributesOfItem(atPath:url.path)
                precondition((attrs[.posixPermissions] as! NSNumber).intValue==0o600)
            } catch {precondition(mode != "success");precondition(!FileManager.default.fileExists(atPath:url.path));precondition(cancels==1)}
            print("PASS hydrate \(mode) reads=\(reads) cancels=\(cancels)")
        }
        let pending = folder.appendingPathComponent("rb-pending-fixture")
        let active = open(pending.path, O_CREAT | O_RDWR | O_EXCL, 0o600)
        precondition(active >= 0 && flock(active, LOCK_EX | LOCK_NB) == 0)
        let held = try cleanupPending(in: folder)
        precondition(held == 0 && FileManager.default.fileExists(atPath: pending.path))
        close(active)
        let abandoned = try cleanupPending(in: folder)
        precondition(abandoned == 1 && !FileManager.default.fileExists(atPath: pending.path))
        print("PASS restart cleanup removes abandoned pending file, preserves active locked file")
        print("PASS shared vectors Unicode UInt64 framing offset paths tuple identity")
    }
}
