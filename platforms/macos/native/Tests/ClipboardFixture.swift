import AppKit
// Explicit synthetic fixture actions. Never prints arbitrary clipboard contents.
let mode = CommandLine.arguments.dropFirst().first ?? ""
let text = "Rustboard TLS Unicode 验证 🦀"
switch mode {
case "check-text":
    guard NSPasteboard.general.string(forType: .string) == text else {exit(1)}
    print("PASS expected synthetic Unicode text on AppKit pasteboard")
case "write-text":
    NSPasteboard.general.clearContents()
    NSPasteboard.general.setString(text, forType: .string)
case "write-file":
    guard CommandLine.arguments.count == 3 else {exit(64)}
    let url = URL(fileURLWithPath: CommandLine.arguments[2])
    NSPasteboard.general.clearContents()
    let item = NSPasteboardItem()
    item.setString(url.absoluteString, forType: .fileURL)
    guard NSPasteboard.general.writeObjects([item]) else {exit(1)}
default:exit(64)
}
