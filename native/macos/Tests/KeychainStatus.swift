// Read-only diagnostic. Never unlocks, changes ACLs, or reads key material.
import Security
@main struct KeychainStatus {
    static func main() {
        var status: SecKeychainStatus = 0
        let result = SecKeychainGetStatus(nil, &status)
        print("SecKeychainGetStatus=\(result) unlocked=\(status & 1 != 0) readable=\(status & 2 != 0) writable=\(status & 4 != 0)")
    }
}
