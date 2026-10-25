use super::*;

fn wait(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "condition timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn pair() -> (Arc<Node>, Arc<Node>, Peer, Peer, TcpListener, TcpListener) {
    let a = Node::new(Identity::generate().unwrap(), vec![]);
    let b = Node::new(Identity::generate().unwrap(), vec![]);
    let la = TcpListener::bind("127.0.0.1:0").unwrap();
    let lb = TcpListener::bind("127.0.0.1:0").unwrap();
    let pa = Peer {
        address: la.local_addr().unwrap(),
        fingerprint: a.identity.pin(),
    };
    let pb = Peer {
        address: lb.local_addr().unwrap(),
        fingerprint: b.identity.pin(),
    };
    a.peers.lock().unwrap().push(pb.clone());
    b.peers.lock().unwrap().push(pa.clone());
    (a, b, pa, pb, la, lb)
}

#[test]
fn incoming_control_pushes_changes_without_idle_frames_or_polling() {
    let (a, b, pa, pb, la, lb) = pair();
    drop(lb);
    let initial = a
        .engine
        .lock()
        .unwrap()
        .local(Content::text("first"), Default::default())
        .unwrap();
    let server = a.serve(la, |_| false).unwrap();
    let mut client = connect(&b.identity, &pa).unwrap();
    write_frame(
        &mut client,
        &Message::Hello {
            protocol: PROTOCOL_VERSION,
            device_id: b.identity.device(),
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame(&mut client).unwrap(),
        Message::Hello { .. }
    ));
    assert!(
        matches!(read_frame(&mut client).unwrap(), Message::Offer { offer } if offer.offer_id == initial.offer_id)
    );
    assert_eq!(a.peer_status(pb.address)["status"], "connected");
    client
        .sock
        .set_read_timeout(Some(Duration::from_millis(1200)))
        .unwrap();
    let idle = read_frame::<_, Message>(&mut client).unwrap_err();
    assert!(idle.chain().any(
        |cause| cause.downcast_ref::<io::Error>().is_some_and(|e| matches!(
            e.kind(),
            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
        ))
    ));
    let next = a
        .engine
        .lock()
        .unwrap()
        .local(Content::text("second"), Default::default())
        .unwrap();
    assert!(
        matches!(read_frame(&mut client).unwrap(), Message::Offer { offer } if offer.offer_id == next.offer_id)
    );
    drop(client);
    wait(|| a.peer_status(pb.address)["status"] == "disconnected");
    a.shutdown();
    b.shutdown();
    server.join().unwrap();
}

#[test]
fn simultaneous_connections_and_large_updates_share_one_control_socket() {
    let (a, b, pa, pb, la, lb) = pair();
    let sa = a.serve(la, |_| false).unwrap();
    let sb = b.serve(lb, |_| false).unwrap();
    a.start_sync();
    b.start_sync();
    wait(|| {
        a.peer_status(pb.address)["status"] == "connected"
            && b.peer_status(pa.address)["status"] == "connected"
    });
    let chosen = |node: &Node, address| {
        node.peer_connections.lock().unwrap()[&address]
            .connection
            .clone()
            .unwrap()
    };
    // Allow the simultaneous outgoing handshakes to finish and retire duplicates.
    std::thread::sleep(Duration::from_millis(200));
    let ca = chosen(&a, pb.address);
    let cb = chosen(&b, pa.address);
    assert_eq!(
        ca.socket.local_addr().unwrap(),
        cb.socket.peer_addr().unwrap()
    );
    assert_eq!(
        ca.socket.peer_addr().unwrap(),
        cb.socket.local_addr().unwrap()
    );
    let (oa, ob) = {
        let mut ea = a.engine.lock().unwrap();
        let mut eb = b.engine.lock().unwrap();
        (
            ea.local(Content::text("a".repeat(MAX_TEXT)), Default::default())
                .unwrap(),
            eb.local(Content::text("b".repeat(MAX_TEXT)), Default::default())
                .unwrap(),
        )
    };
    let winner = if oa.order_key() > ob.order_key() {
        oa
    } else {
        ob
    };
    wait(|| {
        [&a, &b].iter().all(|node| {
            node.engine
                .lock()
                .unwrap()
                .current
                .as_ref()
                .is_some_and(|offer| offer.offer_id == winner.offer_id)
        })
    });
    assert!(Arc::ptr_eq(&ca, &chosen(&a, pb.address)));
    assert!(Arc::ptr_eq(&cb, &chosen(&b, pa.address)));
    let next = b
        .engine
        .lock()
        .unwrap()
        .local(Content::text("after large text"), Default::default())
        .unwrap();
    wait(|| {
        a.engine
            .lock()
            .unwrap()
            .current
            .as_ref()
            .is_some_and(|offer| offer.offer_id == next.offer_id)
    });
    a.shutdown();
    b.shutdown();
    sa.join().unwrap();
    sb.join().unwrap();
}

#[test]
fn stalled_and_invalid_tls_peers_do_not_block_a_healthy_peer() {
    let (a, b, pa, _, la, lb) = pair();
    drop(lb);
    let slow = TcpListener::bind("127.0.0.1:0").unwrap();
    let slow_peer = Peer {
        address: slow.local_addr().unwrap(),
        fingerprint: Identity::generate().unwrap().pin(),
    };
    let bad_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let bad_peer = Peer {
        address: bad_listener.local_addr().unwrap(),
        fingerprint: Identity::generate().unwrap().pin(),
    };
    let bad = Node::new(
        Identity::generate().unwrap(),
        vec![Peer {
            address: "127.0.0.1:1".parse().unwrap(),
            fingerprint: b.identity.pin(),
        }],
    );
    *b.peers.lock().unwrap() = vec![slow_peer, bad_peer.clone(), pa.clone()];
    let sa = a.serve(la, |_| false).unwrap();
    let sbad = bad.serve(bad_listener, |_| false).unwrap();
    let error = connect(&b.identity, &bad_peer).err().unwrap();
    assert_eq!(
        sync_error(&error),
        Some("certificate mismatch"),
        "{error:#?}"
    );
    let offer = a
        .engine
        .lock()
        .unwrap()
        .local(Content::text("healthy"), Default::default())
        .unwrap();
    b.start_sync();
    let (blocked, _) = slow.accept().unwrap();
    let started = Instant::now();
    wait(|| {
        b.engine
            .lock()
            .unwrap()
            .current
            .as_ref()
            .is_some_and(|o| o.offer_id == offer.offer_id)
    });
    assert!(started.elapsed() < Duration::from_secs(2));
    wait(|| b.peer_status(bad_peer.address)["status"] == "error");
    assert_eq!(
        b.peer_status(bad_peer.address)["error"],
        "certificate mismatch"
    );
    a.shutdown();
    b.shutdown();
    bad.shutdown();
    drop(blocked);
    drop(slow);
    sa.join().unwrap();
    sbad.join().unwrap();
}

#[test]
fn three_peer_mesh_delivers_each_change_once() {
    let nodes: Vec<_> = (0..3)
        .map(|_| Node::new(Identity::generate().unwrap(), vec![]))
        .collect();
    let listeners: Vec<_> = (0..3)
        .map(|_| TcpListener::bind("127.0.0.1:0").unwrap())
        .collect();
    let peers: Vec<_> = nodes
        .iter()
        .zip(&listeners)
        .map(|(node, listener)| Peer {
            address: listener.local_addr().unwrap(),
            fingerprint: node.identity.pin(),
        })
        .collect();
    let mut servers = Vec::new();
    for (i, (node, listener)) in nodes.iter().zip(listeners).enumerate() {
        *node.peers.lock().unwrap() = peers
            .iter()
            .enumerate()
            .filter(|(j, _)| i != *j)
            .map(|(_, peer)| peer.clone())
            .collect();
        servers.push(node.serve(listener, |_| false).unwrap());
    }
    for node in &nodes {
        node.start_sync();
    }
    for (i, source) in nodes.iter().enumerate() {
        let offer = source
            .engine
            .lock()
            .unwrap()
            .local(Content::text(format!("peer {i}")), Default::default())
            .unwrap();
        wait(|| {
            nodes.iter().all(|node| {
                node.engine
                    .lock()
                    .unwrap()
                    .current
                    .as_ref()
                    .is_some_and(|current| current.offer_id == offer.offer_id)
            })
        });
    }
    for node in &nodes {
        assert_eq!(node.engine.lock().unwrap().accepted, 2);
        node.shutdown();
    }
    for server in servers {
        server.join().unwrap();
    }
}

#[test]
fn incoming_connection_recovers_during_retry_and_after_peer_restart() {
    let (a, b, pa, pb, la, lb) = pair();
    drop(lb);
    let sa = a.serve(la, |_| false).unwrap();
    a.start_sync();
    wait(|| {
        a.peer_status(pb.address)["retry_in_seconds"]
            .as_u64()
            .is_some_and(|n| n >= 3)
    });
    let started = Instant::now();
    let sb = b
        .serve(TcpListener::bind(pb.address).unwrap(), |_| false)
        .unwrap();
    b.start_sync();
    wait(|| a.peer_status(pb.address)["status"] == "connected");
    assert!(started.elapsed() < Duration::from_secs(2));
    let offer = b
        .engine
        .lock()
        .unwrap()
        .local(Content::text("online"), Default::default())
        .unwrap();
    wait(|| {
        a.engine
            .lock()
            .unwrap()
            .current
            .as_ref()
            .is_some_and(|o| o.offer_id == offer.offer_id)
    });
    b.shutdown();
    sb.join().unwrap();
    wait(|| a.peer_status(pb.address)["status"] == "disconnected");
    let restarted = Node::new(b.identity.clone(), vec![pa]);
    let server = restarted
        .serve(TcpListener::bind(pb.address).unwrap(), |_| false)
        .unwrap();
    restarted.start_sync();
    wait(|| a.peer_status(pb.address)["status"] == "connected");
    a.shutdown();
    restarted.shutdown();
    sa.join().unwrap();
    server.join().unwrap();
}

fn incoming(identity: &Identity, peer: &Peer) -> StreamOwned<ClientConnection, TcpStream> {
    let mut stream = connect(identity, peer).unwrap();
    write_frame(
        &mut stream,
        &Message::Hello {
            protocol: PROTOCOL_VERSION,
            device_id: identity.device(),
        },
    )
    .unwrap();
    assert!(matches!(
        read_frame(&mut stream).unwrap(),
        Message::Hello { .. }
    ));
    stream
}

fn connection(node: &Node, peer: &Peer) -> Arc<Control> {
    node.peer_connections.lock().unwrap()[&peer.address]
        .connection
        .clone()
        .unwrap()
}

fn assert_idle(stream: &mut StreamOwned<ClientConnection, TcpStream>) {
    stream
        .sock
        .set_read_timeout(Some(Duration::from_millis(1200)))
        .unwrap();
    let error = read_frame::<_, Message>(stream).unwrap_err();
    assert!(
        error
            .chain()
            .any(|cause| cause.downcast_ref::<io::Error>().is_some_and(|e| {
                matches!(
                    e.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                )
            })),
        "{error:#}"
    );
}

#[test]
fn incoming_sends_while_outgoing_tls_is_stalled() {
    let (a, b, pa, pb, la, lb) = pair();
    let server = a.serve(la, |_| false).unwrap();
    a.start_sync();
    let (stalled, _) = lb.accept().unwrap();
    let mut stream = incoming(&b.identity, &pa);
    let offer = a
        .engine
        .lock()
        .unwrap()
        .local(Content::text("incoming works"), Default::default())
        .unwrap();
    stream
        .sock
        .set_read_timeout(Some(Duration::from_secs(1)))
        .unwrap();
    let received = read_frame::<_, Message>(&mut stream);
    let still_connecting = a.peer_connections.lock().unwrap()[&pb.address].connecting;
    a.shutdown();
    b.shutdown();
    drop(stalled);
    server.join().unwrap();
    assert!(matches!(received, Ok(Message::Offer { offer: o }) if o.offer_id == offer.offer_id));
    assert!(
        still_connecting,
        "outgoing handshake finished before the check"
    );
}

#[test]
fn busy_retries_after_capacity_is_freed_without_reconnecting() {
    let (a, b, pa, pb, la, lb) = pair();
    let content = Content::Files {
        files: vec![FileEntry {
            directory: false,
            modified_ms: None,
            executable: None,
            file_id: engine::new_id(),
            relative_path: "small.txt".into(),
            size: Some(16),
            version: "test".into(),
            media_type: "text/plain".into(),
        }],
    };
    let mut held = {
        let mut engine = b.engine.lock().unwrap();
        (0..8)
            .map(|_| {
                let offer = engine.local(content.clone(), Default::default()).unwrap();
                engine.retain_batch(&offer.offer_id).unwrap()
            })
            .collect::<Vec<_>>()
    };
    let offer = {
        let mut engine = a.engine.lock().unwrap();
        engine.observe_clock(100).unwrap();
        engine.local(content, Default::default()).unwrap()
    };
    let sa = a.serve(la, |_| false).unwrap();
    let sb = b.serve(lb, |_| false).unwrap();
    a.start_sync();
    b.start_sync();
    wait(|| b.engine.lock().unwrap().clock() == offer.event_sequence);
    wait(|| {
        [&a, &b].iter().all(|node| {
            node.peer_connections
                .lock()
                .unwrap()
                .values()
                .all(|state| !state.connecting && state.connection.is_some())
        })
    });
    let ca = connection(&a, &pb);
    let cb = connection(&b, &pa);
    assert_ne!(
        b.engine.lock().unwrap().current.as_ref().unwrap().offer_id,
        offer.offer_id
    );
    held.pop();
    wait(|| {
        b.engine
            .lock()
            .unwrap()
            .current
            .as_ref()
            .is_some_and(|o| o.offer_id == offer.offer_id)
    });
    assert!(Arc::ptr_eq(&ca, &connection(&a, &pb)));
    assert!(Arc::ptr_eq(&cb, &connection(&b, &pa)));
    assert_eq!(b.engine.lock().unwrap().accepted, 1);
    a.shutdown();
    b.shutdown();
    sa.join().unwrap();
    sb.join().unwrap();
}

#[test]
fn busy_retry_drops_replaced_and_expired_offers() {
    let (a, b, pa, _, la, lb) = pair();
    drop(lb);
    let sa = a.serve(la, |_| false).unwrap();
    let mut stream = incoming(&b.identity, &pa);
    let old = a
        .engine
        .lock()
        .unwrap()
        .local(Content::text("old"), Default::default())
        .unwrap();
    assert!(
        matches!(read_frame(&mut stream).unwrap(), Message::Offer { offer } if offer.offer_id == old.offer_id)
    );
    write_frame(
        &mut stream,
        &Message::OfferRejected {
            offer_id: old.offer_id.clone(),
            code: ErrorCode::Busy,
        },
    )
    .unwrap();
    let next = a
        .engine
        .lock()
        .unwrap()
        .local(Content::text("new"), Default::default())
        .unwrap();
    assert!(
        matches!(read_frame(&mut stream).unwrap(), Message::Offer { offer } if offer.offer_id == next.offer_id)
    );
    // A delayed rejection for the old offer must not resend either offer.
    write_frame(
        &mut stream,
        &Message::OfferRejected {
            offer_id: old.offer_id,
            code: ErrorCode::Busy,
        },
    )
    .unwrap();
    assert_idle(&mut stream);
    write_frame(
        &mut stream,
        &Message::OfferRejected {
            offer_id: next.offer_id,
            code: ErrorCode::Busy,
        },
    )
    .unwrap();
    a.engine
        .lock()
        .unwrap()
        .current
        .as_mut()
        .unwrap()
        .expires_unix_ms = 0;
    assert_idle(&mut stream);
    a.shutdown();
    b.shutdown();
    sa.join().unwrap();
}

#[test]
fn conflicting_connection_keeps_responsive_peer_and_replaces_unresponsive_peer() {
    let (a, b, pa, pb, la, lb) = pair();
    let (a, b, pa, pb, la, lb) = if a.identity.pin() < b.identity.pin() {
        (a, b, pa, pb, la, lb)
    } else {
        (b, a, pb, pa, lb, la)
    };
    let sa = a.serve(la, |_| false).unwrap();
    let sb = b.serve(lb, |_| false).unwrap();
    a.start_sync();
    b.start_sync();
    wait(|| {
        [&a, &b].iter().all(|node| {
            node.peer_connections
                .lock()
                .unwrap()
                .values()
                .all(|state| !state.connecting && state.connection.is_some())
        })
    });
    let old = connection(&a, &pb);
    assert!(old.outbound);
    let mut redundant = incoming(&b.identity, &pa);
    // A responsive preferred connection wins; the authenticated duplicate is closed.
    assert!(read_frame::<_, Message>(&mut redundant).is_err());
    assert!(Arc::ptr_eq(&old, &connection(&a, &pb)));
    let other = connection(&b, &pa);
    let stalled = other.tls.lock().unwrap();
    let mut replacement = incoming(&b.identity, &pa);
    wait(|| !Arc::ptr_eq(&old, &connection(&a, &pb)));
    b.shutdown();
    drop(stalled);
    let offer = a
        .engine
        .lock()
        .unwrap()
        .local(Content::text("recovered"), Default::default())
        .unwrap();
    assert!(
        matches!(read_frame(&mut replacement).unwrap(), Message::Offer { offer: o } if o.offer_id == offer.offer_id)
    );
    assert_eq!(a.peer_status(pb.address)["status"], "connected");
    a.shutdown();
    sa.join().unwrap();
    sb.join().unwrap();
}
