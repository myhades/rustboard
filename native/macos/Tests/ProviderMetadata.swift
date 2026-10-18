import Foundation
import FileProvider
@main struct ProviderMetadataTest {
    static func main() throws {
        let files = (0..<256).map {FileEntry(file_id: UUID().uuidString, relative_path: "file-\($0).bin", size: 1, version: "v1", media_type: "application/octet-stream")}
        let offer = Offer(protocol: 1, source_device: UUID().uuidString, offer_id: UUID().uuidString, event_sequence: 1, expires_unix_ms: UInt64(Date().timeIntervalSince1970 * 1000) + 600000, content: Content(kind: "files", files: files, text: nil))
        try offer.validate()
        let all = files.map {Item(offer, $0)}
        let anchor = try snapshotAnchor(all)
        precondition(anchor.rawValue.count == 32 && anchor.rawValue.count <= 500)
        let reversed = try snapshotAnchor(Array(all.reversed()))
        precondition(reversed == anchor)
        let changed = try snapshotAnchor(Array(all.dropLast()))
        precondition(changed != anchor)
        precondition(Set(all.map {$0.itemIdentifier}).count == 256)
        let other = Offer(protocol: 1, source_device: offer.source_device, offer_id: UUID().uuidString, event_sequence: 2, expires_unix_ms: offer.expires_unix_ms, content: offer.content)
        let first = Item(offer, files[0]), second = Item(other, files[0])
        precondition(first.filename == second.filename)
        precondition(first.parentItemIdentifier != second.parentItemIdentifier)
        precondition(first.parentItemIdentifier == Item(offer).itemIdentifier)
        precondition(Item(offer).parentItemIdentifier == .rootContainer)
        print("PASS same-name files isolated by batch directory")
        print("PASS 256-file metadata: stable 32-byte anchor, changes detectable, unique item IDs")
    }
}
