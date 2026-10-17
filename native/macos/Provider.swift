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
    var itemIdentifier: NSFileProviderItemIdentifier {guard let offer, let file else {return .rootContainer}; return .init(itemID(offer,file))}
    var parentItemIdentifier: NSFileProviderItemIdentifier {.rootContainer}
    var filename: String {file?.relative_path ?? "Rustboard"}
    var contentType: UTType {file == nil ? .folder : (UTType(filenameExtension: (filename as NSString).pathExtension) ?? .data)}
    // Finder propagates uchg for read-only capabilities and fails repeated copy.
    // This is an export namespace: content write-back remains unsupported below.
    var capabilities: NSFileProviderItemCapabilities {file == nil ? [.allowsContentEnumerating] : [.allowsReading, .allowsWriting]}
    var fileSystemFlags: NSFileProviderFileSystemFlags {file == nil ? [.userReadable,.userWritable,.userExecutable] : [.userReadable,.userWritable]}
    var contentPolicy: NSFileProviderContentPolicy {.downloadLazily}
    var documentSize: NSNumber? {file?.size.map {NSNumber(value:$0)}}
    var itemVersion: NSFileProviderItemVersion { .init(contentVersion: Data((file?.version ?? "root").utf8), metadataVersion: Data((offer?.offer_id ?? "root").utf8)) }
}
func items(profile: String) throws -> [Item] {guard let offer=try currentOffer(profile: profile), let files=offer.content.files else {return []};return files.map {Item(offer,$0)}}
func lookup(_ id: NSFileProviderItemIdentifier, profile: String) throws -> Item {
    if id == .rootContainer {return Item()}
    guard let item=try items(profile: profile).first(where: {$0.itemIdentifier==id}) else {throw NSFileProviderError(.noSuchItem)};return item
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
    init(profile: String) {self.profile=profile}
    func invalidate() {}
    func enumerateItems(for observer:NSFileProviderEnumerationObserver,startingAt page:NSFileProviderPage) {
        do {observer.didEnumerate(try items(profile: profile));observer.finishEnumerating(upTo:nil)} catch {observer.finishEnumeratingWithError(error)}
    }
    func enumerateChanges(for observer:NSFileProviderChangeObserver,from anchor:NSFileProviderSyncAnchor) {
        do {
            guard anchor.rawValue.count == 32 || (anchor.rawValue.count == 9 && anchor.rawValue.first == 1) else {
                throw NSFileProviderError(.syncAnchorExpired)
            }
            let current = (try? items(profile: profile)) ?? []
            let ids = Set(current.map {$0.itemIdentifier.rawValue})
            let acknowledged: UInt64? = anchor.rawValue.count == 9 ? anchor.rawValue.dropFirst().reduce(UInt64(0)) {($0 << 8) | UInt64($1)} : nil
            let (retired, generation) = try retiredItems(profile: profile, acknowledged: acknowledged)
            let deleted = Set(retired).subtracting(ids)
            observer.didDeleteItems(withIdentifiers: deleted.map {NSFileProviderItemIdentifier($0)})
            observer.didUpdate(current)
            var token = Data([1])
            for shift in stride(from: 56, through: 0, by: -8) {token.append(UInt8(truncatingIfNeeded: generation >> shift))}
            observer.finishEnumeratingChanges(upTo: NSFileProviderSyncAnchor(token), moreComing: false)
        } catch {observer.finishEnumeratingWithError(error)}
    }
    func currentSyncAnchor(completionHandler:@escaping(NSFileProviderSyncAnchor?)->Void) {
        completionHandler(try? snapshotAnchor((try? items(profile: profile)) ?? []))
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
        log.notice("fetch_start")
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
                if removed > 0 {log.notice("stale_pending_removed count=\(removed)")}
                let pending = directory.appendingPathComponent("rb-pending-" + UUID().uuidString)
                let url = directory.appendingPathComponent("rb-ready-" + UUID().uuidString)
                stage = "authenticated_ipc"
                let session = try IPCSession(profile: self.profile)
                stage = "hydrate"
                try hydrate(offer,file,to:pending,progress:progress,call: session.call,commit: {try FileManager.default.moveItem(at: $0, to: url)})
                guard !progress.isCancelled else {try? FileManager.default.removeItem(at:url);throw WireError.cancelled}
                log.notice("fetch_complete bytes=\(size)")
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
        guard identifier == .rootContainer || identifier == .workingSet else {throw NSFileProviderError(.noSuchItem)};return Enumerator(profile: profile)
    }
    func createItem(basedOn itemTemplate:NSFileProviderItem,fields:NSFileProviderItemFields,contents url:URL?,options:NSFileProviderCreateItemOptions,request:NSFileProviderRequest,completionHandler:@escaping(NSFileProviderItem?,NSFileProviderItemFields,Bool,Error?)->Void)->Progress {completionHandler(nil,[],false,CocoaError(.featureUnsupported));return Progress()}
    func modifyItem(_ item:NSFileProviderItem,baseVersion version:NSFileProviderItemVersion,changedFields:NSFileProviderItemFields,contents newContents:URL?,options:NSFileProviderModifyItemOptions,request:NSFileProviderRequest,completionHandler:@escaping(NSFileProviderItem?,NSFileProviderItemFields,Bool,Error?)->Void)->Progress {completionHandler(nil,[],false,CocoaError(.featureUnsupported));return Progress()}
    func deleteItem(identifier:NSFileProviderItemIdentifier,baseVersion version:NSFileProviderItemVersion,options:NSFileProviderDeleteItemOptions,request:NSFileProviderRequest,completionHandler:@escaping(Error?)->Void)->Progress {completionHandler(CocoaError(.featureUnsupported));return Progress()}
}
