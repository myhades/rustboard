use super::*;
#[test]
fn binary_frame_bounds_and_truncation() {
    let message = Message::Data {
        request_id: u64::MAX,
        offset: 9007199254740993,
        eof: true,
        bytes: vec![255; MAX_CHUNK],
    };
    let mut encoded = Vec::new();
    write_v2_response(&mut encoded, &message).unwrap();
    let buffer = Vec::with_capacity(MAX_CHUNK);
    let allocation = buffer.as_ptr();
    let reply = read_v2_data(&mut &encoded[..], MAX_CHUNK, buffer).unwrap();
    assert_eq!(reply.bytes.as_ptr(), allocation);
    assert_eq!(reply.request_id, u64::MAX);
    assert_eq!(reply.offset, 9007199254740993);
    assert!(reply.eof);
    assert_eq!(reply.bytes, vec![255; MAX_CHUNK]);
    assert!(read_v2_data(&mut &encoded[..], MAX_CHUNK - 1, Vec::new()).is_err());
    for n in [0, 3, 4, 20, encoded.len() - 1] {
        assert!(read_v2_data(&mut &encoded[..n], MAX_CHUNK, Vec::new()).is_err());
    }
    encoded[20] = 2;
    assert!(read_v2_data(&mut &encoded[..], MAX_CHUNK, Vec::new()).is_err());
    assert!(read_v2_data(&mut &u32::MAX.to_be_bytes()[..], READ_WINDOW, Vec::new()).is_err());
}

#[test]
fn negotiated_binary_window_is_lazy_bounded_and_revocable() {
    let a = Node::new(Identity::generate().unwrap(), vec![]);
    let b = Node::new(Identity::generate().unwrap(), vec![]);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let peer = Peer {
        address: listener.local_addr().unwrap(),
        fingerprint: a.identity.pin(),
    };
    a.peers.lock().unwrap().push(Peer {
        address: peer.address,
        fingerprint: b.identity.pin(),
    });
    b.peers.lock().unwrap().push(peer.clone());
    let source = Arc::new(PatternSource::new(READ_WINDOW as u64 * 3));
    let id = engine::new_id();
    let file = FileEntry {
        directory: false,
        modified_ms: None,
        executable: None,
        file_id: id.clone(),
        relative_path: "test.bin".into(),
        size: Some(source.len()),
        version: "v1".into(),
        media_type: "application/octet-stream".into(),
    };
    let offer = a
        .engine
        .lock()
        .unwrap()
        .local(
            Content::Files {
                files: vec![file.clone()],
            },
            std::collections::HashMap::from([(id, source.clone() as Arc<dyn ReadAt>)]),
        )
        .unwrap();
    let server = a.serve(listener, |_| false).unwrap();
    crate::test_network::sync(&a, &b);
    assert!(
        a.local_ipc(IpcOperation::GetFileOffers).unwrap()["offers"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        b.local_ipc(IpcOperation::GetFileOffers).unwrap()["offers"][0]["offer_id"],
        offer.offer_id
    );
    assert_eq!(source.bytes_read.load(Ordering::Relaxed), 0);
    let remote = RemoteSource::new(b.clone(), peer, offer.offer_id.clone(), file);
    remote.read_at(0, &mut [0; 4096]).unwrap();
    assert_eq!(source.bytes_read.load(Ordering::Relaxed), 4096);
    a.engine
        .lock()
        .unwrap()
        .local(
            Content::Text {
                text: "source changed".into(),
                html: None,
                rtf: None,
            },
            Default::default(),
        )
        .unwrap();
    crate::test_network::sync(&a, &b);
    b.engine
        .lock()
        .unwrap()
        .local(
            Content::Text {
                text: "receiver changed".into(),
                html: None,
                rtf: None,
            },
            Default::default(),
        )
        .unwrap();
    // Exercise a real server idle disconnect after clipboard replacement.
    std::thread::sleep(Duration::from_secs(6));
    let mut bytes = vec![0; MAX_CHUNK];
    remote.read_at(4096, &mut bytes).unwrap();
    assert_eq!(
        source.bytes_read.load(Ordering::Relaxed),
        4096 + READ_WINDOW as u64
    );
    remote.read_at(4096 + MAX_CHUNK as u64, &mut bytes).unwrap();
    assert_eq!(bytes[0], ((4096 + MAX_CHUNK) % 251) as u8);
    assert_eq!(
        source.bytes_read.load(Ordering::Relaxed),
        4096 + READ_WINDOW as u64
    );
    assert!(remote.connection.lock().unwrap().bytes.len() <= READ_WINDOW);
    b.engine.lock().unwrap().withdraw();
    assert!(
        remote
            .read_at(4096 + 2 * MAX_CHUNK as u64, &mut bytes)
            .is_err()
    );
    assert!(remote.connection.lock().unwrap().bytes.is_empty());
    drop(remote);
    // Dropping the native owner closes the retention channel, even though
    // the independent read channel may have already reconnected.
    for _ in 0..100 {
        if a.engine.lock().unwrap().retained_offers().is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(a.engine.lock().unwrap().retained_offers().is_empty());
    assert!(
        b.local_ipc(IpcOperation::GetFileOffer {
            offer_id: offer.offer_id,
        })
        .unwrap()["offer"]
            .is_null()
    );
    a.shutdown();
    b.shutdown();
    server.join().unwrap();
}
#[test]
fn mutual_tls_offer_lazy_read_and_untrusted_rejection() {
    let a = Node::new(Identity::generate().unwrap(), vec![]);
    let b = Node::new(Identity::generate().unwrap(), vec![]);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let peer = Peer {
        address: listener.local_addr().unwrap(),
        fingerprint: a.identity.pin(),
    };
    a.peers.lock().unwrap().push(Peer {
        address: "127.0.0.1:1".parse().unwrap(),
        fingerprint: b.identity.pin(),
    });
    b.peers.lock().unwrap().push(peer.clone());
    let source = Arc::new(PatternSource::new(4096));
    let id = engine::new_id();
    let file = FileEntry {
        directory: false,
        modified_ms: None,
        executable: None,
        file_id: id.clone(),
        relative_path: "sample.bin".into(),
        size: Some(4096),
        version: "pattern-v1".into(),
        media_type: "application/octet-stream".into(),
    };
    let offer = a
        .engine
        .lock()
        .unwrap()
        .local(
            Content::Files {
                files: vec![file.clone()],
            },
            std::collections::HashMap::from([(id, source.clone() as Arc<dyn ReadAt>)]),
        )
        .unwrap();
    let server = a.serve(listener, |_| false).unwrap();
    crate::test_network::sync(&a, &b);
    assert_eq!(source.bytes_read.load(Ordering::Relaxed), 0);
    let ipc_read = || IpcOperation::ReadBinary {
        background: false,
        request_id: 7,
        offer_id: offer.offer_id.clone(),
        file_id: file.file_id.clone(),
        version: file.version.clone(),
        offset: 250,
        length: 100,
    };
    let remote = RemoteSource::new(
        b.clone(),
        peer.clone(),
        offer.offer_id.clone(),
        file.clone(),
    );
    let mut bytes = [0; 100];
    assert_eq!(remote.read_at(250, &mut bytes).unwrap(), 100);
    assert_eq!(&bytes[..3], &[250, 0, 1]);
    for _ in 0..2 {
        let reply = b.local_ipc(ipc_read()).unwrap();
        assert_eq!(reply["response"]["bytes"][0], 250);
    }
    assert_eq!(b.ipc_connections.lock().unwrap().len(), 1);
    assert_eq!(source.bytes_read.load(Ordering::Relaxed), 300);
    // Existing cached TLS sessions must still fail after receiver-side unpair.
    b.peers.lock().unwrap().clear();
    assert!(b.local_ipc(ipc_read()).is_err());
    assert_eq!(source.bytes_read.load(Ordering::Relaxed), 300);
    b.peers.lock().unwrap().push(peer.clone());
    b.local_ipc(IpcOperation::Cancel { request_id: 7 }).unwrap();
    assert!(b.ipc_connections.lock().unwrap().is_empty());
    let stranger = Identity::generate().unwrap();
    let outcome = connect(&stranger, &peer).and_then(|mut unknown| {
        write_frame(
            &mut unknown,
            &Message::Hello {
                protocol: PROTOCOL_VERSION,
                device_id: stranger.device(),
            },
        )?;
        read_frame::<_, Message>(&mut unknown)
    });
    assert!(outcome.is_err());
    a.peers.lock().unwrap().clear();
    assert!(remote.read_at(0, &mut bytes).is_err());
    a.shutdown();
    b.shutdown();
    server.join().unwrap();
}

#[test]
fn source_failure_mid_window_does_not_return_partial_data() {
    struct FailingSource;
    impl ReadAt for FailingSource {
        fn len(&self) -> u64 {
            (3 * MAX_CHUNK) as u64
        }
        fn read_at(&self, offset: u64, bytes: &mut [u8]) -> Result<usize> {
            ensure!(offset < (2 * MAX_CHUNK) as u64, "source changed");
            bytes.fill(42);
            Ok(bytes.len())
        }
    }
    let a = Node::new(Identity::generate().unwrap(), vec![]);
    let b = Node::new(Identity::generate().unwrap(), vec![]);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let peer = Peer {
        address: listener.local_addr().unwrap(),
        fingerprint: a.identity.pin(),
    };
    a.peers.lock().unwrap().push(Peer {
        address: peer.address,
        fingerprint: b.identity.pin(),
    });
    b.peers.lock().unwrap().push(peer.clone());
    let source = Arc::new(FailingSource);
    let id = engine::new_id();
    let file = FileEntry {
        directory: false,
        modified_ms: None,
        executable: None,
        file_id: id.clone(),
        relative_path: "test.bin".into(),
        size: Some(source.len()),
        version: "v1".into(),
        media_type: "application/octet-stream".into(),
    };
    let offer = a
        .engine
        .lock()
        .unwrap()
        .local(
            Content::Files {
                files: vec![file.clone()],
            },
            std::collections::HashMap::from([(id, source.clone() as Arc<dyn ReadAt>)]),
        )
        .unwrap();
    let server = a.serve(listener, |_| false).unwrap();
    crate::test_network::sync(&a, &b);
    let remote = RemoteSource::new(b.clone(), peer, offer.offer_id.clone(), file);
    remote.read_at(0, &mut [0; MAX_CHUNK]).unwrap();

    let mut bytes = [99; MAX_CHUNK];
    assert!(remote.read_at(MAX_CHUNK as u64, &mut bytes).is_err());
    assert!(bytes.iter().all(|byte| *byte == 99));
    assert!(remote.connection.lock().unwrap().bytes.is_empty());
    drop(remote);
    a.shutdown();
    b.shutdown();
    server.join().unwrap();
}

#[test]
fn failed_platform_recycle_keeps_the_offer_and_is_not_retried() {
    static RECYCLES: AtomicUsize = AtomicUsize::new(0);
    let source = Node::new(Identity::generate().unwrap(), vec![]);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let peer = Peer {
        address: listener.local_addr().unwrap(),
        fingerprint: source.identity.pin(),
    };
    let target = Node::new(Identity::generate().unwrap(), vec![peer.clone()]);
    source.peers.lock().unwrap().push(Peer {
        address: peer.address,
        fingerprint: target.identity.pin(),
    });
    let file = FileEntry {
        file_id: engine::new_id(),
        relative_path: "file".into(),
        directory: false,
        modified_ms: None,
        executable: None,
        size: Some(4),
        version: "v".into(),
        media_type: "application/octet-stream".into(),
    };
    let offer = source
        .engine
        .lock()
        .unwrap()
        .local_files(
            vec![file.clone()],
            std::collections::HashMap::from([(
                file.file_id.clone(),
                Arc::new(PatternSource::new(4)) as Arc<dyn ReadAt>,
            )]),
            vec![PathBuf::from("file")],
            true,
        )
        .unwrap();
    let server = source
        .serve(listener, |paths| {
            assert_eq!(paths, [PathBuf::from("file")]);
            RECYCLES.fetch_add(1, Ordering::SeqCst);
            false
        })
        .unwrap();
    crate::test_network::sync(&source, &target);
    assert!(target.recycle_remote(&offer.offer_id).is_err());
    assert_eq!(RECYCLES.load(Ordering::SeqCst), 0);
    let remote = RemoteSource::new(target.clone(), peer, offer.offer_id.clone(), file);
    assert_eq!(remote.read_at(0, &mut [0; 4]).unwrap(), 4);
    target.recycle_remote(&offer.offer_id).unwrap();
    target.recycle_remote(&offer.offer_id).unwrap();
    assert_eq!(RECYCLES.load(Ordering::SeqCst), 1);
    assert_eq!(
        source
            .engine
            .lock()
            .unwrap()
            .current
            .as_ref()
            .unwrap()
            .offer_id,
        offer.offer_id
    );
    drop(remote);
    source.shutdown();
    target.shutdown();
    server.join().unwrap();
}
