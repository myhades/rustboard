import FileProvider
import UniformTypeIdentifiers
import os
import CryptoKit

private let fetchSlots = DispatchSemaphore(value: 2)
private let log = Logger(subsystem: "org.rustboard", category: "provider")
final class Item: NSObject, NSFileProviderItem {
    let offer: Offer?
    let file: FileEntry?
    init(_ offer: Offer? = nil, _ file: FileEntry? = nil) {self.offer=offer; self.file=file}
    var itemIdentifier: NSFileProviderItemIdentifier {guard let offer else {return .rootContainer}; guard let file else {return .init("batch:" + offer.offer_id)}; return .init(itemID(offer,file))}
    var parentItemIdentifier: NSFileProviderItemIdentifier {guard let offer, file != nil else {return .rootContainer}; return .init("batch:" + offer.offer_id)}
    var filename: String {file?.relative_path ?? offer?.offer_id ?? "Rustboard"}
    var contentType: UTType {file == nil ? .folder : (UTType(filenameExtension: (filename as NSString).pathExtension) ?? .data)}
    // Finder propagates uchg for read-only capabilities and fails repeated copy.
    // This is an export namespace: content write-back remains unsupported below.
    var capabilities: NSFileProviderItemCapabilities {file == nil ? [.allowsContentEnumerating] : [.allowsReading, .allowsWriting]}
    var fileSystemFlags: NSFileProviderFileSystemFlags {file == nil ? [.userReadable,.userWritable,.userExecutable] : [.userReadable,.userWritable]}
    var contentPolicy: NSFileProviderContentPolicy {.downloadLazily}
    var documentSize: NSNumber? {file?.size.map {NSNumber(value:$0)}}
    var itemVersion: NSFileProviderItemVersion { .init(contentVersion: Data((file?.version ?? "root").utf8), metadataVersion: Data((offer?.offer_id ?? "root").utf8)) }
}
func items(profile: String, parent: NSFileProviderItemIdentifier = .rootContainer) throws -> [Item] {
    let offers = try fileOffers(profile: profile)
    if parent == .rootContainer {return offers.map {Item($0)}}
    if parent == .workingSet {return offers.flatMap {o in [Item(o)] + (o.content.files ?? []).map {Item(o,$0)}}}
    guard parent.rawValue.hasPrefix("batch:"), let offer = offers.first(where: {"batch:" + $0.offer_id == parent.rawValue}) else {throw NSFileProviderError(.noSuchItem)}
    return (offer.content.files ?? []).map {Item(offer,$0)}
}
func lookup(_ id: NSFileProviderItemIdentifier, profile: String) throws -> Item {
    if id == .rootContainer {return Item()}
    let parts=id.rawValue.split(separator:":")
    let offerID: String
    if parts.count == 2 && parts[0] == "batch" {offerID=String(parts[1])}
    else if parts.count == 5 && parts[0] == "rb1" {offerID=String(parts[2])}
    else {throw NSFileProviderError(.noSuchItem)}
    guard let offer=try fileOffer(offerID, profile:profile) else {throw NSFileProviderError(.noSuchItem)}
    if parts.count == 2 {return Item(offer)}
    guard let file=offer.content.files?.first(where:{itemID(offer,$0)==id.rawValue}) else {throw NSFileProviderError(.noSuchItem)}
    return Item(offer,file)
}
// File Provider vendor tokens must be <=500 bytes. Deletion identities live
// in the private retirement ledger; the anchor is always a 32-byte digest.
func snapshotAnchor(_ items: [Item]) throws -> NSFileProviderSyncAnchor {
    let data = try JSONEncoder().encode(items.map {$0.itemIdentifier.rawValue}.sorted())
    guard data.count <= frameLimit else {throw WireError.invalid}
    return NSFileProviderSyncAnchor(Data(SHA256.hash(data: data)))
}
final class Enumerator: NSObject, NSFileProviderEnumerator {
    let profile: String
    let parent: NSFileProviderItemIdentifier
    init(profile: String, parent: NSFileProviderItemIdentifier) {self.profile=profile;self.parent=parent}
    func invalidate() {}
    func enumerateItems(for observer:NSFileProviderEnumerationObserver,startingAt page:NSFileProviderPage) {
        do {observer.didEnumerate(try items(profile: profile, parent: parent));observer.finishEnumerating(upTo:nil)} catch {observer.finishEnumeratingWithError(error)}
    }
    func enumerateChanges(for observer:NSFileProviderChangeObserver,from anchor:NSFileProviderSyncAnchor) {
        do {
            guard anchor.rawValue.count == 32 || (anchor.rawValue.count == 9 && anchor.rawValue.first == 1) else {
                throw NSFileProviderError(.syncAnchorExpired)
            }
            let current = try items(profile: profile, parent: parent)
            let ids = Set(current.map {$0.itemIdentifier.rawValue})
            let acknowledged: UInt64? = anchor.rawValue.count == 9 ? anchor.rawValue.dropFirst().reduce(UInt64(0)) {($0 << 8) | UInt64($1)} : nil
            let (retired, generation) = try retiredItems(profile: profile, acknowledged: parent == .workingSet ? acknowledged : nil)
            let deleted = Set(retired.filter { id in
                if parent == .workingSet {return true}
                if parent == .rootContainer {return id.hasPrefix("batch:")}
                let offerID=String(parent.rawValue.dropFirst("batch:".count))
                let parts=id.split(separator:":")
                return parts.count == 5 && String(parts[2]) == offerID
            }).subtracting(ids)
            observer.didDeleteItems(withIdentifiers: deleted.map {NSFileProviderItemIdentifier($0)})
            observer.didUpdate(current)
            var token = Data([1])
            for shift in stride(from: 56, through: 0, by: -8) {token.append(UInt8(truncatingIfNeeded: generation >> shift))}
            observer.finishEnumeratingChanges(upTo: NSFileProviderSyncAnchor(token), moreComing: false)
        } catch {observer.finishEnumeratingWithError(error)}
    }
    func currentSyncAnchor(completionHandler:@escaping(NSFileProviderSyncAnchor?)->Void) {
        completionHandler(try? snapshotAnchor((try? items(profile: profile, parent: parent)) ?? []))
    }
}
final class FileProviderExtension: NSObject, NSFileProviderReplicatedExtension {
    let domain:NSFileProviderDomain
    var profile: String {String(domain.identifier.rawValue.dropFirst("rustboard-v1-".count))}
    required init(domain:NSFileProviderDomain) {self.domain=domain;super.init()}
    func invalidate() {}
    func item(for identifier:NSFileProviderItemIdentifier,request:NSFileProviderRequest,completionHandler:@escaping(NSFileProviderItem?,Error?)->Void)->Progress {
        do {completionHandler(try lookup(identifier, profile: self.profile),nil)} catch {completionHandler(nil,error)};return Progress()
    }
    func fetchContents(for identifier:NSFileProviderItemIdentifier,version requestedVersion:NSFileProviderItemVersion?,request:NSFileProviderRequest,completionHandler:@escaping(URL?,NSFileProviderItem?,Error?)->Void)->Progress {
        let progress=Progress(totalUnitCount:0)
        log.debug("fetch_start background=\(request.isSystemRequest)")
        DispatchQueue.global().async {
            while fetchSlots.wait(timeout:.now()+0.1) != .success {
                if progress.isCancelled {completionHandler(nil,nil,CocoaError(.userCancelled));return}
            }
            defer {fetchSlots.signal()}
            var stage = "lookup"
            do {
                let item=try lookup(identifier, profile: self.profile)
                guard let offer=item.offer, let file=item.file, let size=file.size,
                      requestedVersion == nil || requestedVersion == item.itemVersion,
                      let manager=NSFileProviderManager(for:self.domain) else {throw WireError.stale}
                progress.totalUnitCount=Int64(size)
                stage = "temporary_directory"
                let directory = try manager.temporaryDirectoryURL()
                let removed = try cleanupPending(in: directory)
                if removed > 0 {log.debug("stale_pending_removed count=\(removed)")}
                let pending = directory.appendingPathComponent("rb-pending-" + UUID().uuidString)
                let url = directory.appendingPathComponent("rb-ready-" + UUID().uuidString)
                stage = "authenticated_ipc"
                let session = try IPCSession(profile: self.profile)
                guard try session.call(Operation(type: "retain", offer_id: offer.offer_id)).ok else {throw WireError.stale}
                stage = "hydrate"
                try hydrate(offer,file,to:pending,progress:progress,background:request.isSystemRequest,call: session.call,commit: {try FileManager.default.moveItem(at: $0, to: url)})
                guard !progress.isCancelled else {try? FileManager.default.removeItem(at:url);throw WireError.cancelled}
                log.debug("fetch_complete bytes=\(size)")
                completionHandler(url,item,nil)
            } catch {
                // Log only the stage and typed error code, never paths or file content.
                let ns = error as NSError
                log.error("fetch_failed stage=\(stage, privacy: .public) domain=\(ns.domain, privacy: .public) code=\(ns.code)")
                if case SecurityFailure.stage(let detail, let code) = error {
                    log.error("fetch_failure_detail stage=\(detail, privacy: .public) code=\(code)")
                }
                completionHandler(nil,nil,NSFileProviderError(.serverUnreachable))
            }
        };return progress
    }
    func enumerator(for identifier:NSFileProviderItemIdentifier,request:NSFileProviderRequest)throws->NSFileProviderEnumerator {
        guard identifier == .rootContainer || identifier == .workingSet || identifier.rawValue.hasPrefix("batch:") else {throw NSFileProviderError(.noSuchItem)};return Enumerator(profile: profile, parent: identifier)
    }
    func createItem(basedOn itemTemplate:NSFileProviderItem,fields:NSFileProviderItemFields,contents url:URL?,options:NSFileProviderCreateItemOptions,request:NSFileProviderRequest,completionHandler:@escaping(NSFileProviderItem?,NSFileProviderItemFields,Bool,Error?)->Void)->Progress {completionHandler(nil,[],false,CocoaError(.featureUnsupported));return Progress()}
    func modifyItem(_ item:NSFileProviderItem,baseVersion version:NSFileProviderItemVersion,changedFields:NSFileProviderItemFields,contents newContents:URL?,options:NSFileProviderModifyItemOptions,request:NSFileProviderRequest,completionHandler:@escaping(NSFileProviderItem?,NSFileProviderItemFields,Bool,Error?)->Void)->Progress {completionHandler(nil,[],false,CocoaError(.featureUnsupported));return Progress()}
    func deleteItem(identifier:NSFileProviderItemIdentifier,baseVersion version:NSFileProviderItemVersion,options:NSFileProviderDeleteItemOptions,request:NSFileProviderRequest,completionHandler:@escaping(Error?)->Void)->Progress {completionHandler(CocoaError(.featureUnsupported));return Progress()}
}
