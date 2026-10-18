use rustboard_core::*;
use sha2::{Digest, Sha256};
#[test]
fn shared_mac_linux_windows_vectors() {
    let vectors: serde_json::Value =
        serde_json::from_str(include_str!("../../../tests/protocol/vectors.json")).unwrap();
    let offer: Offer = serde_json::from_value(vectors["offer"].clone()).unwrap();
    offer.validate(1).unwrap();
    let _: Message = serde_json::from_value(vectors["read"].clone()).unwrap();
    let _: IpcRequest = serde_json::from_value(vectors["ipc"].clone()).unwrap();
    let data: Message = serde_json::from_value(vectors["data"].clone()).unwrap();
    if let Message::Data { offset, bytes, .. } = data {
        let mut actual = vec![0; bytes.len()];
        PatternSource::new(4096)
            .read_at(offset, &mut actual)
            .unwrap();
        assert_eq!(actual, bytes);
    } else {
        panic!("wrong vector")
    }
    let mut buffer = [0; 4096];
    PatternSource::new(4096).read_at(0, &mut buffer).unwrap();
    assert_eq!(
        format!("{:x}", Sha256::digest(buffer)),
        vectors["hashes"][0]["sha256"].as_str().unwrap()
    );
    let mut encoded = Vec::new();
    write_frame(&mut encoded, &offer).unwrap();
    let decoded: Offer = read_frame(&mut &encoded[..]).unwrap();
    assert_eq!(offer, decoded);
    for path in vectors["invalid_paths"].as_array().unwrap() {
        assert!(validate_path(path.as_str().unwrap()).is_err());
    }
}
