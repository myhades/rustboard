import FileProvider
import UniformTypeIdentifiers
import os
import CryptoKit

private let fetchSlots = DispatchSemaphore(value: 2)
private let log = Logger(subsystem: "org.rustboard", category: "provider")
private let metadataLock = NSLock()
private var metadataCache: [String: Offer] = [:]
// Offers are immutable. Content reads still acquire a live Agent lease.
private func metadata(_ id: String, profile: String) throws -> Offer? {
    let key = profile + "/" + id
    metadataLock.lock(); let cached = metadataCache[key]; metadataLock.unlock()
    if let cached {return cached}
    guard let offer = try fileOffer(id, profile: profile) else {return nil}
    metadataLock.lock()
    if metadataCache.count >= 8 {metadataCache.removeAll()}
    metadataCache[key] = offer
    metadataLock.unlock()
    return offer
}
final class Item: NSObject, NSFileProviderItem {
    let offer: Offer?
    let file: FileEntry?
    init(_ offer: Offer? = nil, _ file: FileEntry? = nil) {self.offer=offer; self.file=file}
    var itemIdentifier: NSFileProviderItemIdentifier {guard let offer else {return .rootContainer}; guard let file else {return .init("batch:" + offer.offer_id)}; return .init(itemID(offer,file))}
    var parentItemIdentifier: NSFileProviderItemIdentifier {
        guard let offer, let file else {return .rootContainer}
        let path = (file.relative_path as NSString).deletingLastPathComponent
        if !path.isEmpty, let parent = offer.content.files?.first(where: {$0.relative_path == path && $0.directory}) {return .init(itemID(offer,parent))}
        return .init("batch:" + offer.offer_id)
    }
    var isDirectory: Bool {file == nil || file!.directory}
    var contentModificationDate: Date? {file?.modified_ms.map {Date(timeIntervalSince1970: Double($0) / 1000)}}
    var filename: String {file.map {($0.relative_path as NSString).lastPathComponent} ?? offer?.offer_id ?? "Rustboard"}
    var contentType: UTType {isDirectory ? .folder : (UTType(filenameExtension: (filename as NSString).pathExtension) ?? .data)}
    // Finder propagates uchg for read-only capabilities and fails repeated copy.
    // This is an export namespace: content write-back remains unsupported below.
    var capabilities: NSFileProviderItemCapabilities {file == nil ? [.allowsReading, .allowsWriting] : [.allowsReading, .allowsWriting, .allowsDeleting]}
    var fileSystemFlags: NSFileProviderFileSystemFlags {isDirectory || file?.executable == true ? [.userReadable,.userWritable,.userExecutable] : [.userReadable,.userWritable]}
    var contentPolicy: NSFileProviderContentPolicy {.downloadLazily}
    var documentSize: NSNumber? {file?.size.map {NSNumber(value:$0)}}
    var itemVersion: NSFileProviderItemVersion { .init(contentVersion: Data((file?.version ?? "root").utf8), metadataVersion: Data((offer?.offer_id ?? "root").utf8)) }
}
func items(profile: String, parent: NSFileProviderItemIdentifier = .rootContainer) throws -> [Item] {
    let offers: [Offer]
    if parent == .rootContainer || parent == .workingSet {
        do {offers = try fileOffers(profile: profile)} catch {
            metadataLock.lock(); metadataCache = metadataCache.filter {!$0.key.hasPrefix(profile + "/")}; metadataLock.unlock()
            throw error
        }
        let live = Set(offers.map {profile + "/" + $0.offer_id})
        metadataLock.lock()
        metadataCache = metadataCache.filter {!$0.key.hasPrefix(profile + "/") || live.contains($0.key)}
        for offer in offers {metadataCache[profile + "/" + offer.offer_id] = offer}
        metadataLock.unlock()
    } else {
        let parts = parent.rawValue.split(separator: ":")
        let id: String
        if parts.count == 2 && parts[0] == "batch" {id = String(parts[1])}
        else if parts.count == 5 && parts[0] == "rb1" {id = String(parts[2])}
        else {throw NSFileProviderError(.noSuchItem)}
        guard let offer = try metadata(id, profile: profile) else {throw NSFileProviderError(.noSuchItem)}
        offers = [offer]
    }
    if parent == .rootContainer {return offers.map {Item($0)}}
    if parent == .workingSet {return offers.flatMap {o in [Item(o)] + (o.content.files ?? []).map {Item(o,$0)}}}
    if let offer = offers.first(where: {"batch:" + $0.offer_id == parent.rawValue}) {
        return (offer.content.files ?? []).filter {!$0.relative_path.contains("/")}.map {Item(offer,$0)}
    }
    for offer in offers {
        if let folder = offer.content.files?.first(where: {$0.directory && itemID(offer,$0) == parent.rawValue}) {
            return (offer.content.files ?? []).filter {($0.relative_path as NSString).deletingLastPathComponent == folder.relative_path}.map {Item(offer,$0)}
        }
    }
    throw NSFileProviderError(.noSuchItem)
}
func lookup(_ id: NSFileProviderItemIdentifier, profile: String) throws -> Item {
    if id == .rootContainer {return Item()}
    let parts=id.rawValue.split(separator:":")
    let offerID: String
    if parts.count == 2 && parts[0] == "batch" {offerID=String(parts[1])}
    else if parts.count == 5 && parts[0] == "rb1" {offerID=String(parts[2])}
    else {throw NSFileProviderError(.noSuchItem)}
    guard let offer=try metadata(offerID, profile:profile) else {throw NSFileProviderError(.noSuchItem)}
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
                return false
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
    func invalidate() {
        metadataLock.lock(); metadataCache = metadataCache.filter {!$0.key.hasPrefix(profile + "/")}; metadataLock.unlock()
    }
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
                guard let offer=item.offer, let file=item.file, !file.directory, let size=file.size,
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
                if offer.cut && !request.isSystemRequest {
                    _ = try? session.call(Operation(type: "file_received", offer_id: offer.offer_id, file_id: file.file_id))
                }
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
        guard try identifier == .rootContainer || identifier == .workingSet || lookup(identifier, profile: profile).isDirectory else {throw NSFileProviderError(.noSuchItem)};return Enumerator(profile: profile, parent: identifier)
    }
    func createItem(basedOn itemTemplate:NSFileProviderItem,fields:NSFileProviderItemFields,contents url:URL?,options:NSFileProviderCreateItemOptions,request:NSFileProviderRequest,completionHandler:@escaping(NSFileProviderItem?,NSFileProviderItemFields,Bool,Error?)->Void)->Progress {completionHandler(nil,[],false,CocoaError(.featureUnsupported));return Progress()}
    func modifyItem(_ item:NSFileProviderItem,baseVersion version:NSFileProviderItemVersion,changedFields:NSFileProviderItemFields,contents newContents:URL?,options:NSFileProviderModifyItemOptions,request:NSFileProviderRequest,completionHandler:@escaping(NSFileProviderItem?,NSFileProviderItemFields,Bool,Error?)->Void)->Progress {completionHandler(nil,[],false,CocoaError(.featureUnsupported));return Progress()}
    func deleteItem(identifier:NSFileProviderItemIdentifier,baseVersion version:NSFileProviderItemVersion,options:NSFileProviderDeleteItemOptions,request:NSFileProviderRequest,completionHandler:@escaping(Error?)->Void)->Progress {
        let progress = Progress()
        DispatchQueue.global().async {
            if let item = try? lookup(identifier, profile: self.profile), let offer = item.offer, let file = item.file, item.itemVersion == version {
                _ = try? ipc(Operation(type: "move_item", offer_id: offer.offer_id, file_id: file.file_id), profile: self.profile)
            }
            completionHandler(nil)
        }
        return progress
    }
}
