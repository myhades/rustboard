import AppKit
@main struct ClipboardTest {
    static func main() throws {
        rbInit()
        let text="Rustboard native test 中文 🦀"
        let payload=String(data:try JSONSerialization.data(withJSONObject:["text":text]),encoding:.utf8)!
        precondition(payload.withCString {rbPublish($0)} == 0)
        precondition(NSPasteboard.general.string(forType:.string)==text)
        precondition(rbCapture()==nil)
        rbClearOwned();precondition(NSPasteboard.general.string(forType:.string)==nil)
        NSPasteboard.general.clearContents();NSPasteboard.general.setString("external synthetic text",forType:.string)
        let raw=rbCapture()!;let data=Data(String(cString:raw).utf8);free(raw)
        let obj=try JSONSerialization.jsonObject(with:data) as! [String:String]
        precondition(obj["kind"]=="text" && obj["text"]=="external synthetic text")
        precondition(rbCapture()==nil)
        rbClearOwned();precondition(NSPasteboard.general.string(forType:.string)=="external synthetic text")
        let url=FileManager.default.temporaryDirectory.appendingPathComponent("rustboard-synthetic-"+UUID().uuidString+".bin")
        try Data([1,2,3]).write(to:url);defer {try? FileManager.default.removeItem(at:url)}
        NSPasteboard.general.clearContents();NSPasteboard.general.writeObjects([url as NSURL])
        let rawFile=rbCapture()!;let fileData=Data(String(cString:rawFile).utf8);free(rawFile)
        let fileObject=try JSONSerialization.jsonObject(with:fileData) as! [String:Any]
        precondition(fileObject["kind"] as? String == "files")
        precondition(fileObject["paths"] as? [String] == [url.path])
        precondition(rbAdoptOwned(Int64(NSPasteboard.general.changeCount) + 1) == -1)
        precondition(rbAdoptOwned(Int64(NSPasteboard.general.changeCount)) == 0)
        precondition(rbCapture() == nil)
        precondition(rbAdoptFiles(Int64(NSPasteboard.general.changeCount), 1) == 0)
        precondition(rbAdoptFiles(Int64(NSPasteboard.general.changeCount), 2) == -1)
        precondition(NSPasteboard.general.types?.isEmpty != false)
        rbClearOwned(); precondition(NSPasteboard.general.types?.isEmpty != false)
        // Only replace the synthetic clipboard this test owns.
        NSPasteboard.general.clearContents()
        print("PASS AppKit Unicode publication, changeCount origin suppression, external text and file URL metadata capture")
    }
}
