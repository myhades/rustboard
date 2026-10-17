import Foundation
import CryptoKit
@main struct LargeHydration {
    static func main() throws {
        struct Vector:Decodable {struct Hash:Decodable {let size:UInt64;let sha256:String};let hashes:[Hash]}
        let v=try JSONDecoder().decode(Vector.self,from:Data(contentsOf:URL(fileURLWithPath:CommandLine.arguments[1])))
        for h in v.hashes {
            let file=FileEntry(file_id:UUID().uuidString,relative_path:"synthetic.bin",size:h.size,version:"pattern-v1",media_type:"application/octet-stream")
            let offer=Offer(protocol:1,source_device:UUID().uuidString,offer_id:UUID().uuidString,event_sequence:42,expires_unix_ms:UInt64(Date().timeIntervalSince1970*1000)+600000,content:Content(kind:"files",files:[file],text:nil))
            let url=FileManager.default.temporaryDirectory.appendingPathComponent("rustboard-test-"+UUID().uuidString)
            var reads=0
            let started=Date()
            precondition(reads==0)
            try hydrate(offer,file,to:url,progress:Progress(totalUnitCount:Int64(h.size))) {op in
                if op.type == "get_offer" {return Reply(ok:true,protocol:1,offer:offer,response:nil)}
                guard op.type == "read" else {throw WireError.invalid}
                reads += 1
                let n=min(UInt64(op.length!),h.size-op.offset!)
                let bytes=(0..<n).map {UInt8((op.offset!+$0)%251)}
                return Reply(ok:true,protocol:1,offer:nil,response:DataMessage(type:"data",request_id:op.request_id!,offset:op.offset!,bytes:bytes,eof:op.offset!+n==h.size))
            }
            let handle=try FileHandle(forReadingFrom:url);var hash=SHA256()
            while let data=try autoreleasepool(invoking: {try handle.read(upToCount:65536)}), !data.isEmpty {hash.update(data:data)}
            try handle.close();try FileManager.default.removeItem(at:url)
            let actual=hash.finalize().map {String(format:"%02x",$0)}.joined()
            precondition(actual==h.sha256)
            print("PASS injected hydration bytes=\(h.size) reads=\(reads) sha256=\(actual) seconds=\(Date().timeIntervalSince(started))")
        }
    }
}
