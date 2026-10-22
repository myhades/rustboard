import AppKit
import ImageIO
import IOKit.pwr_mgt

private var observed = -1
private var owned = -1
@_cdecl("rb_trash") public func rbTrash(_ path: UnsafePointer<CChar>) -> Int32 {
    autoreleasepool {
        do {
            try FileManager.default.trashItem(at: URL(fileURLWithPath: String(cString: path)), resultingItemURL: nil)
            return 0
        } catch {return -1}
    }
}
@_cdecl("rb_init") public func rbInit() {
    _ = NSApplication.shared
    NSApp.setActivationPolicy(.prohibited)
    observed = NSPasteboard.general.changeCount
}
@_cdecl("rb_pump") public func rbPump() {
    RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.05))
}
@_cdecl("rb_capture") public func rbCapture() -> UnsafeMutablePointer<CChar>? {
    let board = NSPasteboard.general
    let sequence = board.changeCount
    guard sequence != observed else { return nil }
    observed = sequence
    guard sequence != owned else { return nil }
    var value: [String: Any] = ["kind": "unsupported"]
    // Read file URL metadata only, never promise/file contents.
    if let urls = board.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL], !urls.isEmpty {
        if urls.count <= 10000 { value = ["kind": "files", "paths": urls.map(\.path)] }
    } else if !(board.types?.contains(.html) ?? false), !(board.types?.contains(.rtf) ?? false),
              board.types?.contains(.png) == true || board.types?.contains(.tiff) == true || board.types?.contains(.init("public.jpeg")) == true {
        value = ["kind": "image"]
    } else if board.types?.contains(.string) == true || board.types?.contains(.html) == true || board.types?.contains(.rtf) == true {
        let text = board.string(forType: .string) ?? ""
        guard text.utf8.count <= 4194304, !text.contains("\0") else {return nil}
        value = ["kind": "text", "text": text]
        if let html = board.string(forType: .html), html.utf8.count <= 4194304 {value["html"] = html}
        if let rtf = board.data(forType: .rtf), rtf.count <= 4194304 {value["rtf"] = rtf.base64EncodedString()}
    }
    guard board.changeCount == sequence, let data = try? JSONSerialization.data(withJSONObject: value), let string = String(data: data, encoding: .utf8) else { return nil }
    return strdup(string)
}
@_cdecl("rb_publish") public func rbPublish(_ json: UnsafePointer<CChar>) -> Int32 {
    guard let data = String(cString: json).data(using: .utf8), let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return -1 }
    let board = NSPasteboard.general
    let objects: [NSPasteboardWriting]
    if let text = obj["text"] as? String {
        let item = NSPasteboardItem()
        item.setString(text, forType: .string)
        if let html = obj["html"] as? String {item.setString(html, forType: .html)}
        if let rtf = obj["rtf"] as? String, let bytes = Data(base64Encoded: rtf) {item.setData(bytes, forType: .rtf)}
        objects = [item]
    }
    else if let paths = obj["paths"] as? [String] { objects = paths.map { NSURL(fileURLWithPath: $0) } }
    else { return -1 }
    board.clearContents()
    let ok = board.writeObjects(objects)
    owned = board.changeCount; observed = owned
    return ok ? 0 : -1
}

@_cdecl("rb_change_count") public func rbChangeCount() -> Int64 {Int64(NSPasteboard.general.changeCount)}
@_cdecl("rb_clear_owned") public func rbClearOwned() {
    let board=NSPasteboard.general
    if board.changeCount == owned {board.clearContents();owned=board.changeCount;observed=owned}
}

@_cdecl("rb_adopt_owned") public func rbAdoptOwned(_ sequence: Int64) -> Int32 {
    guard Int64(NSPasteboard.general.changeCount) == sequence else {return -1}
    owned = Int(sequence); observed = owned; return 0
}

// Validate the pasteboard from the Agent process after the sandboxed host exits.
// writeObjects can report success before pboard rejects individual sandbox grants.
@_cdecl("rb_adopt_files") public func rbAdoptFiles(_ sequence: Int64, _ count: Int64) -> Int32 {
    let board = NSPasteboard.general
    guard Int64(board.changeCount) == sequence else {return -1}
    let urls = board.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL] ?? []
    guard Int64(board.changeCount) == sequence else {return -1}
    guard Int64(urls.count) == count, count > 0 else {
        board.clearContents(); owned = board.changeCount; observed = owned
        return -1
    }
    return rbAdoptOwned(sequence)
}

// Keep PNG/JPEG bytes; only TIFF needs a system-codec conversion for other platforms.
// Called on the same AppKit thread immediately after rb_capture returned image.
@_cdecl("rb_capture_image_file") public func rbCaptureImageFile(_ stem: UnsafePointer<CChar>) -> Int32 {
    let board = NSPasteboard.general
    guard board.changeCount == observed else {return -1}
    for (type, ext, code) in [(NSPasteboard.PasteboardType.png, "png", Int32(1)), (.init("public.jpeg"), "jpg", Int32(2)), (.tiff, "png", Int32(1))] {
        guard board.types?.contains(type) == true else {continue}
        guard var data = board.data(forType: type), !data.isEmpty, data.count <= 64 * 1024 * 1024 else {return -1}
        if type == .tiff {
            guard let source = CGImageSourceCreateWithData(data as CFData, nil),
                  let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
                  let width = properties[kCGImagePropertyPixelWidth] as? Int,
                  let height = properties[kCGImagePropertyPixelHeight] as? Int,
                  width > 0, height > 0, width <= 16777216 / height,
                  let image = CGImageSourceCreateImageAtIndex(source, 0, [kCGImageSourceShouldCache: false] as CFDictionary) else {return -1}
            let output = NSMutableData()
            guard let destination = CGImageDestinationCreateWithData(output, "public.png" as CFString, 1, nil) else {return -1}
            CGImageDestinationAddImage(destination, image, nil)
            guard CGImageDestinationFinalize(destination), output.length <= 64 * 1024 * 1024 else {return -1}
            data = output as Data
        }
        let url = URL(fileURLWithPath: String(cString: stem) + "." + ext)
        do {
            try data.write(to: url, options: .withoutOverwriting)
            guard board.changeCount == observed else {try? FileManager.default.removeItem(at: url); return -1}
            return code
        } catch {return -1}
    }
    return -1
}


// Match caffeinate -u -t 5 without launching a subprocess.
@_cdecl("rb_wake_on_receive") public func rbWakeOnReceive() -> Int32 {
    var assertion: IOPMAssertionID = 0
    return IOPMAssertionCreateWithDescription(
        "UserIsActive" as CFString,
        "Rustboard received clipboard content" as CFString,
        nil, nil, nil, 5,
        kIOPMAssertionTimeoutActionRelease as CFString,
        &assertion)
}

@_cdecl("rb_publish_image") public func rbPublishImage(_ bytes: UnsafePointer<UInt8>, _ count: Int, _ jpeg: Int32) -> Int32 {
    guard count > 0, count <= 64 * 1024 * 1024 else {return -1}
    let item = NSPasteboardItem()
    item.setData(Data(bytes: bytes, count: count), forType: jpeg == 0 ? .png : .init("public.jpeg"))
    let board = NSPasteboard.general
    board.clearContents()
    let ok = board.writeObjects([item])
    owned = board.changeCount; observed = owned
    return ok ? 0 : -1
}
