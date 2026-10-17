import AppKit

private var observed = -1
private var owned = -1
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
        if urls.count <= 256 { value = ["kind": "files", "paths": urls.map(\.path)] }
    } else if let text = board.string(forType: .string), text.utf8.count <= 262144, !text.contains("\0") {
        value = ["kind": "text", "text": text]
    }
    guard board.changeCount == sequence, let data = try? JSONSerialization.data(withJSONObject: value), let string = String(data: data, encoding: .utf8) else { return nil }
    return strdup(string)
}
@_cdecl("rb_publish") public func rbPublish(_ json: UnsafePointer<CChar>) -> Int32 {
    guard let data = String(cString: json).data(using: .utf8), let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else { return -1 }
    let board = NSPasteboard.general
    let objects: [NSPasteboardWriting]
    if let text = obj["text"] as? String { objects = [text as NSString] }
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
