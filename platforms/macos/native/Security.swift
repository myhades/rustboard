import Foundation
import Security
import LocalAuthentication
import Darwin

// No PID-only identity check: the kernel audit token includes PID version.
func signingTeam() throws -> String {
    var code: SecCode?
    guard SecCodeCopySelf([], &code) == errSecSuccess, let code else { throw SecurityFailure.denied }
    var staticCode: SecStaticCode?
    guard SecCodeCopyStaticCode(code, [], &staticCode) == errSecSuccess, let staticCode else {throw SecurityFailure.denied}
    var info: CFDictionary?
    guard SecCodeCopySigningInformation(staticCode, SecCSFlags(rawValue: kSecCSSigningInformation), &info) == errSecSuccess,
          let team = (info as? [String: Any])?[kSecCodeInfoTeamIdentifier as String] as? String,
          team.count == 10, team.allSatisfy({ $0.isASCII && $0.isLetter || $0.isNumber }) else { throw SecurityFailure.denied }
    return team
}
enum SecurityFailure: Error { case denied; case stage(String, Int32) }
func groupDirectory() throws -> URL {
    let group = try signingTeam() + ".org.rustboard.shared"
    guard let url = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: group) else { throw SecurityFailure.denied }
    return url
}
func authenticateSocket(_ fd: Int32, server: Bool) throws {
    var uid: uid_t = 0, gid: gid_t = 0
    guard getpeereid(fd, &uid, &gid) == 0, uid == getuid() else { throw SecurityFailure.denied }
    var token = audit_token_t()
    var length = socklen_t(MemoryLayout.size(ofValue: token))
    guard getsockopt(fd, SOL_LOCAL, LOCAL_PEERTOKEN, &token, &length) == 0,
          length == MemoryLayout.size(ofValue: token) else { throw SecurityFailure.denied }
    let audit = withUnsafeBytes(of: &token) { Data($0) }
    var guest: SecCode?
    var attributes: [CFString: Any] = [kSecGuestAttributeAudit: audit]
    // Sandboxed clients must validate the live Agent without opening its executable
    // outside their container. Agent has no Info.plist; retain audit-token identity.
    if server {attributes[kSecGuestAttributeDynamicCode] = true}
    let guestStatus = SecCodeCopyGuestWithAttributes(nil, attributes as CFDictionary, [], &guest)
    if guestStatus != errSecSuccess {NSLog("Rustboard IPC guest validation status=%d",guestStatus)}
    guard guestStatus == errSecSuccess,
          let guest else { throw SecurityFailure.denied }
    let identifiers = server ? ["org.rustboard.agent"] : ["org.rustboard.cli", "org.rustboard.host", "org.rustboard.provider"]
    let ids = identifiers.map { "identifier \"\($0)\"" }.joined(separator: " or ")
    let requirement = "anchor apple generic and certificate leaf[subject.OU] = \"\(try signingTeam())\" and (\(ids))"
    var required: SecRequirement?
    let parsed = SecRequirementCreateWithString(requirement as CFString, [], &required)
    guard parsed == errSecSuccess else {throw SecurityFailure.stage("requirement",parsed)}
    let checked = SecCodeCheckValidity(guest, [], required)
    guard checked == errSecSuccess else {throw SecurityFailure.stage("signature",checked)}
}
@_cdecl("rb_authenticate") public func rbAuthenticate(_ fd: Int32, _ server: Int32) -> Int32 {
    do { try authenticateSocket(fd, server: server != 0); return 0 } catch { return -1 }
}
@_cdecl("rb_group_path") public func rbGroupPath() -> UnsafeMutablePointer<CChar>? {
    guard let url = try? groupDirectory() else { return nil }; return strdup(url.path)
}

// Device TLS key storage, independent of the developer's code-signing identity.
// Default macOS login Keychain, non-synchronizing, default creator application ACL.
private func identityQuery(_ account: UnsafePointer<CChar>) -> [String: Any] {
    [kSecClass as String:kSecClassGenericPassword,
     kSecAttrService as String:"org.rustboard.device-identity",
     kSecAttrAccount as String:String(cString:account),
     kSecAttrSynchronizable as String:false]
}
@_cdecl("rb_keychain_load") public func rbKeychainLoad(_ account: UnsafePointer<CChar>, _ bytes: UnsafeMutablePointer<UInt8>, _ capacity: Int, _ length: UnsafeMutablePointer<Int>) -> Int32 {
    var query=identityQuery(account)
    query[kSecReturnData as String]=true
    query[kSecMatchLimit as String]=kSecMatchLimitOne
    let context=LAContext();context.interactionNotAllowed=true
    query[kSecUseAuthenticationContext as String]=context
    var result: CFTypeRef?
    let status=SecItemCopyMatching(query as CFDictionary,&result)
    guard status == errSecSuccess else {return status}
    guard let data=result as? Data, !data.isEmpty, data.count <= capacity else {return errSecDecode}
    data.copyBytes(to:bytes,count:data.count);length.pointee=data.count
    return errSecSuccess
}
@_cdecl("rb_keychain_store") public func rbKeychainStore(_ account: UnsafePointer<CChar>, _ bytes: UnsafePointer<UInt8>, _ length: Int) -> Int32 {
    guard length > 0 && length <= 16384 else {return errSecParam}
    var query=identityQuery(account)
    query[kSecValueData as String]=Data(bytes:bytes,count:length)
    query[kSecAttrLabel as String]="Rustboard device TLS identity"
    let context=LAContext();context.interactionNotAllowed=true
    query[kSecUseAuthenticationContext as String]=context
    let status=SecItemAdd(query as CFDictionary,nil)
    if status == errSecDuplicateItem {
        // Never overwrite an existing secret. Verify before accepting an interrupted retry.
        var existing=[UInt8](repeating:0,count:16384);var count=0
        let read=rbKeychainLoad(account,&existing,16384,&count)
        guard read == errSecSuccess else {return read}
        return Data(existing.prefix(count)) == Data(bytes:bytes,count:length) ? errSecSuccess : errSecDuplicateItem
    }
    return status
}
