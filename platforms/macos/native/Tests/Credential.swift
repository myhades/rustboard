import Foundation
import Security
// Explicit opt-in probe: creates and removes one synthetic Keychain item.
@main struct CredentialTest {
    static func main() {
        let account="rustboard-test-"+UUID().uuidString
        let secret:[UInt8]=[1,3,5,7,9]
        account.withCString { name in
            let created=rbKeychainStore(name,secret,secret.count)
            guard created == errSecSuccess else {print("Keychain probe failed OSStatus",created);exit(1)}
            defer {
                let query:[String:Any]=[kSecClass as String:kSecClassGenericPassword,kSecAttrService as String:"org.rustboard.device-identity",kSecAttrAccount as String:account,kSecAttrSynchronizable as String:false]
                precondition(SecItemDelete(query as CFDictionary)==errSecSuccess)
            }
            var result=[UInt8](repeating:0,count:32);var length=0
            precondition(rbKeychainLoad(name,&result,32,&length)==errSecSuccess)
            precondition(Array(result.prefix(length))==secret)
            precondition(rbKeychainStore(name,secret,secret.count)==errSecSuccess)
            precondition(rbKeychainStore(name,[2,4,6],3)==errSecDuplicateItem)
            print("PASS Keychain create/read, idempotent retry, conflicting key refused; synthetic item removed on exit")
        }
    }
}
