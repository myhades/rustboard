import AppKit
import FileProvider

let app=NSApplication.shared
app.setActivationPolicy(.prohibited)
let profile=ProcessInfo.processInfo.environment["RUSTBOARD_PROFILE"] ?? "default"
let domain=NSFileProviderDomain(identifier:.init("rustboard-v1-"+profile),displayName:profile == "default" ? "Rustboard" : "Rustboard-" + profile)
domain.isHidden=true
func finish(_ error:Error?) {
    if let error {
        if case SecurityFailure.stage(let stage,let code) = error {fputs("IPC stage=\(stage) code=\(code)\n",stderr)}
        else {let ns=error as NSError;fputs("File Provider failure domain=\(ns.domain) code=\(ns.code)\n",stderr)}
    }
    exit(error == nil ? 0 : 1)
}
func indexURL() throws -> URL {try groupDirectory().appendingPathComponent(profile).appendingPathComponent("presented-batches.json")}
func readIndex() throws -> [Offer] {
    let url=try indexURL()
    guard FileManager.default.fileExists(atPath:url.path) else {return []}
    let attrs=try FileManager.default.attributesOfItem(atPath:url.path)
    guard (attrs[.size] as? NSNumber)?.intValue ?? Int.max <= frameLimit else {throw WireError.invalid}
    return try JSONDecoder().decode([Offer].self,from:Data(contentsOf:url))
}
func writeIndex(_ offers: [Offer]) throws {
    let data=try JSONEncoder().encode(offers)
    guard data.count <= frameLimit else {throw WireError.invalid}
    let url=try indexURL();try data.write(to:url,options:[.atomic])
    guard chmod(url.path,0o600)==0 else {throw WireError.unavailable}
}
func saveIndex(_ offer: Offer) throws {
    var offers=try readIndex();offers.removeAll {$0.offer_id == offer.offer_id};offers.append(offer)
    try writeIndex(offers)
}
// Only expired batches are retired. New clipboard contents never evict live files.
func evictPrevious(_ completion:@escaping(Error?)->Void) {
    do {
        let old=try readIndex()
        guard !old.isEmpty else {completion(nil);return}
        let retained=Set(try fileOffers(profile:profile).map {$0.offer_id})
        let expired=old.filter {!retained.contains($0.offer_id)}
        guard !expired.isEmpty else {completion(nil);return}
        let deleted=expired.flatMap {o in (o.content.files ?? []).map {itemID(o,$0)} + ["batch:" + o.offer_id]}
        _ = try retiredItems(profile:profile,adding:deleted)
        guard let manager=NSFileProviderManager(for:domain) else {throw WireError.unavailable}
        let group=DispatchGroup(), lock=NSLock();var failed=false
        for offer in expired {for file in offer.content.files ?? [] {
            group.enter();manager.evictItem(identifier:.init(itemID(offer,file))) {error in
                if let error, (error as NSError).code != NSFileProviderError.noSuchItem.rawValue {lock.lock();failed=true;lock.unlock()}
                group.leave()
            }
        }}
        group.notify(queue:.main) {
            if failed {completion(WireError.unavailable);return}
            do {try writeIndex(old.filter {retained.contains($0.offer_id)});manager.signalEnumerator(for:.rootContainer) {_ in manager.signalEnumerator(for:.workingSet,completionHandler:completion)}} catch {completion(error)}
        }
    } catch {completion(error)}
}
let mode=CommandLine.arguments.dropFirst().first ?? "status"
switch mode {
case "register":NSFileProviderManager.getDomainsWithCompletionHandler {domains,error in
    if let error {finish(error);return}
    if domains.contains(where: {$0.identifier == domain.identifier}) {finish(nil);return}
    NSFileProviderManager.add(domain,completionHandler:finish)
}
case "revoke":evictPrevious(finish)
case "remove":NSFileProviderManager.remove(domain,completionHandler:finish)
case "status":NSFileProviderManager.getDomainsWithCompletionHandler {domains,error in
    print(domains.filter {$0.identifier==domain.identifier}.map {"\($0.identifier.rawValue) hidden=\($0.isHidden)"}.joined(separator:"\n"));finish(error)
}
case "urls":
    do {
        guard let offer=try currentOffer(), offer.offer_id==CommandLine.arguments.dropFirst(2).first,
              let files=offer.content.files, let manager=NSFileProviderManager(for:domain) else {throw WireError.stale}
        evictPrevious {error in
            guard error == nil else {finish(error);return}
            do {try saveIndex(offer)} catch {finish(error);return}
            manager.signalEnumerator(for:.rootContainer) {_ in }
            manager.signalEnumerator(for:.workingSet) {_ in }
            var urls=[URL]();let lock=NSLock();let group=DispatchGroup();var failed=false
            for file in files {
                group.enter()
                manager.getUserVisibleURL(for:.init(itemID(offer,file))) {url,error in
                    lock.lock();if let url, error == nil {urls.append(url)} else {failed=true;if let error {let ns=error as NSError;fputs("getUserVisibleURL domain=\(ns.domain) code=\(ns.code)\n",stderr)}};lock.unlock();group.leave()
                }
            }
            group.notify(queue:.main) {
                guard !failed, urls.count==files.count else {finish(WireError.unavailable);return}
                do {
                    try saveIndex(offer)
                    if let raw = CommandLine.arguments.dropFirst(3).first, let expected = Int(raw) {
                        guard let current = try currentOffer(), current.offer_id == offer.offer_id,
                              current.source_device == offer.source_device,
                              NSPasteboard.general.changeCount == expected else {throw WireError.stale}
                        let accessed = urls.filter {$0.startAccessingSecurityScopedResource()}
                        defer {accessed.forEach {$0.stopAccessingSecurityScopedResource()}}
                        NSPasteboard.general.clearContents()
                        // Materialize each URL representation before this short-lived host exits.
                        // NSURL may provide additional pasteboard representations lazily.
                        let items = urls.map { url -> NSPasteboardItem in
                            let item = NSPasteboardItem()
                            item.setString(url.absoluteString, forType: .fileURL)
                            return item
                        }
                        guard NSPasteboard.general.writeObjects(items),
                              NSPasteboard.general.pasteboardItems?.count == files.count else {throw WireError.unavailable}
                        let data = try JSONSerialization.data(withJSONObject: ["change_count": NSPasteboard.general.changeCount])
                        FileHandle.standardOutput.write(data)
                        // pboard validates sandbox grants asynchronously. Retain scopes while
                        // it consumes them; the Agent independently verifies all URL entries
                        // after our exit and clears/retries any incomplete publication.
                        RunLoop.current.run(until: Date(timeIntervalSinceNow: 0.5))
                    } else {
                        let data = try JSONSerialization.data(withJSONObject: ["paths": urls.map(\.path)])
                        FileHandle.standardOutput.write(data)
                    }
                    finish(nil)
                } catch {finish(error)}
            }
        }
    } catch {finish(error)}
default:exit(64)
}
Timer.scheduledTimer(withTimeInterval:25,repeats:false) {_ in finish(WireError.unavailable)}
app.run()
