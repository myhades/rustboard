import AppKit
import CryptoKit
let args=CommandLine.arguments
let board=NSPasteboard.general
switch args[1] {
case "png", "tiff":
    let data=try Data(contentsOf: URL(fileURLWithPath:args[2]))
    let type:NSPasteboard.PasteboardType=args[1] == "png" ? .png : .tiff
    let image:NSData
    if args[1] == "tiff" {image=NSBitmapImageRep(data:data)!.tiffRepresentation! as NSData} else {image=data as NSData}
    board.clearContents();precondition(board.setData(image as Data,forType:type));print("PASS synthetic image published")
case "files":
    let urls=board.readObjects(forClasses:[NSURL.self],options:[.urlReadingFileURLsOnly:true]) as? [URL] ?? []
    precondition(!urls.isEmpty)
    for url in urls {
        if args.count>2 {
            let data=try Data(contentsOf:url)
            print(url.lastPathComponent, data.count, SHA256.hash(data:data).map{String(format:"%02x",$0)}.joined())
        } else {print(url.lastPathComponent)}
    }
default:exit(64)
}
