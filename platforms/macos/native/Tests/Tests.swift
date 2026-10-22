@main enum NativeTests {
    static func main() throws {
        try ProtocolVectors.run()
        try BinaryFraming.run()
        try ProviderMetadataTest.run()
    }
}
