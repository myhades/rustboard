use crate::core::transport::{Node, pairing};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    path::Path,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};
struct Session {
    id: String,
    phase: String,
    peer: Option<SocketAddr>,
    pin: String,
    approved: bool,
    peer_approved: bool,
    cancel: bool,
    done: bool,
    error: String,
    deadline: Instant,
    touched: Instant,
    socket: Option<TcpStream>,
}
static SESSION: OnceLock<Mutex<Option<Arc<Mutex<Session>>>>> = OnceLock::new();
fn alive(s: &Session) -> bool {
    !s.cancel && Instant::now() < s.deadline && s.touched.elapsed() < Duration::from_secs(10)
}
pub fn control(node: &Arc<Node>, dir: &Path, args: &[String]) -> Result<Value> {
    let mut slot = SESSION.get_or_init(|| Mutex::new(None)).lock().unwrap();
    if args[0] == "pair-begin" {
        ensure!(args.len() <= 2, "invalid pairing request");
        if let Some(s) = slot.as_ref() {
            ensure!(s.lock().unwrap().done, "pairing already active");
        }
        let config = crate::app::config::load(dir)?;
        let own: SocketAddr = config["listen"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("listening disabled"))?
            .parse()?;
        let remote = if args.len() == 2 {
            Some(args[1].parse::<SocketAddr>()?)
        } else {
            None
        };
        let listener = if remote.is_none() {
            let mut address = own;
            address.set_port(
                own.port()
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("pairing port overflow"))?,
            );
            let l = TcpListener::bind(address)?;
            l.set_nonblocking(true)?;
            Some(l)
        } else {
            None
        };
        let state = Arc::new(Mutex::new(Session {
            id: crate::core::engine::new_id(),
            phase: "waiting".into(),
            peer: None,
            pin: String::new(),
            approved: false,
            peer_approved: false,
            cancel: false,
            done: false,
            error: String::new(),
            deadline: Instant::now() + Duration::from_secs(120),
            touched: Instant::now(),
            socket: None,
        }));
        let id = state.lock().unwrap().id.clone();
        *slot = Some(state.clone());
        let watch = state.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(200));
                let mut s = watch.lock().unwrap();
                if s.done {
                    break;
                }
                if !alive(&s) {
                    s.cancel = true;
                    if let Some(sock) = &s.socket {
                        let _ = sock.shutdown(Shutdown::Both);
                    }
                }
            }
        });
        let own_pin = node.identity.pin();
        let node = node.clone();
        let dir = dir.to_path_buf();
        std::thread::spawn(move || {
            let run = || -> Result<()> {
                let (mut ch, ip) = if let Some(mut a) = remote {
                    a.set_port(
                        a.port()
                            .checked_add(1)
                            .ok_or_else(|| anyhow::anyhow!("pairing port overflow"))?,
                    );
                    let sock = TcpStream::connect_timeout(&a, Duration::from_secs(3))?;
                    {
                        let mut s = state.lock().unwrap();
                        ensure!(alive(&s), "pairing cancelled or expired");
                        s.socket = Some(sock.try_clone()?);
                    }
                    (
                        pairing::connect(&node.identity, sock, &|| alive(&state.lock().unwrap()))?,
                        a.ip(),
                    )
                } else {
                    let l = listener.unwrap();
                    loop {
                        ensure!(
                            alive(&state.lock().unwrap()),
                            "pairing cancelled or expired"
                        );
                        match l.accept() {
                            Ok((sock, a)) => {
                                {
                                    let mut s = state.lock().unwrap();
                                    ensure!(alive(&s), "pairing cancelled or expired");
                                    s.socket = Some(sock.try_clone()?);
                                }
                                break (
                                    pairing::accept(&node.identity, sock, &|| {
                                        alive(&state.lock().unwrap())
                                    })?,
                                    a.ip(),
                                );
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                std::thread::sleep(Duration::from_millis(100))
                            }
                            Err(e) => return Err(e.into()),
                        }
                    }
                };
                ch.send(u64::from(own.port()))?;
                let port = u16::try_from(ch.receive(|| alive(&state.lock().unwrap()))?)?;
                ensure!(port > 0, "invalid advertised port");
                let peer = SocketAddr::new(ip, port);
                if let Some(expected) = remote {
                    ensure!(peer == expected, "peer port mismatch");
                }
                ensure!(ch.pin != node.identity.pin(), "cannot pair self");
                {
                    let mut s = state.lock().unwrap();
                    s.pin = ch.pin.clone();
                    s.peer = Some(peer);
                    s.phase = "confirm".into();
                }
                ch.confirm(|peer_approved| {
                    let mut s = state.lock().unwrap();
                    ensure!(alive(&s), "pairing cancelled or expired");
                    s.peer_approved = peer_approved;
                    Ok(s.approved)
                })?;
                ensure!(
                    alive(&state.lock().unwrap()),
                    "pairing cancelled or expired"
                );
                let clock = node.engine.lock().unwrap().clock();
                ch.send(clock)?;
                let clock = ch.receive(|| alive(&state.lock().unwrap()))?;
                node.engine.lock().unwrap().observe_clock(clock)?;
                crate::app::config::save_peer(
                    &dir,
                    &node.identity.pin(),
                    crate::core::transport::Peer {
                        address: peer,
                        fingerprint: ch.pin.clone(),
                    },
                )?;
                // Both persist before reporting completion. Disconnect may leave one side saved; retry is idempotent.
                ch.send(2)?;
                ensure!(
                    ch.receive(|| alive(&state.lock().unwrap()))? == 2,
                    "peer declined pairing"
                );
                Ok(())
            };
            let result = run();
            let mut s = state.lock().unwrap();
            s.done = true;
            s.phase = if result.is_ok() { "saved" } else { "failed" }.into();
            if let Err(e) = result {
                s.error = e.to_string();
            }
            s.socket = None;
        });
        return Ok(json!({"ok":true,"session":id,"fingerprint":own_pin,"listen":own}));
    }
    ensure!(args.len() == 2, "session required");
    let state = slot
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no pairing session"))?;
    let mut s = state.lock().unwrap();
    ensure!(s.id == args[1], "stale pairing session");
    s.touched = Instant::now();
    match args[0].as_str() {
        "pair-poll" => {}
        "pair-approve" => {
            ensure!(
                s.phase == "confirm" && alive(&s),
                "not awaiting confirmation"
            );
            s.approved = true;
            s.phase = "saving".into();
        }
        "pair-cancel" => {
            s.cancel = true;
            if let Some(sock) = &s.socket {
                let _ = sock.shutdown(Shutdown::Both);
            }
        }
        _ => anyhow::bail!("unknown pairing operation"),
    }
    Ok(
        json!({"ok":true,"phase":s.phase,"peer":s.peer,"fingerprint":s.pin,"peer_approved":s.peer_approved,"error":s.error}),
    )
}
