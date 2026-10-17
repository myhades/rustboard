import Foundation
import Security
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
