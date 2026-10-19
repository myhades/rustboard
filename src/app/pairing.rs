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
            .ok_or_else(|| anyhow::anyhow!("Listening is disabled. Set listen to 0.0.0.0:45452 in rustboard config, then run rustboard restart."))?
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
                    (pairing::connect(&node.identity, a)?, a.ip())
                } else {
                    let l = listener.unwrap();
                    loop {
                        ensure!(
                            alive(&state.lock().unwrap()),
                            "pairing cancelled or expired"
                        );
                        match l.accept() {
                            Ok((sock, a)) => {
                                break (pairing::accept(&node.identity, sock)?, a.ip());
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                                std::thread::sleep(Duration::from_millis(100))
                            }
                            Err(e) => return Err(e.into()),
                        }
                    }
                };
                {
                    state.lock().unwrap().socket = Some(ch.socket.try_clone()?);
                }
                ch.send(own.port())?;
                let port = ch.receive()?;
                ensure!(port > 0, "invalid advertised port");
                let peer = SocketAddr::new(ip, port);
                if let Some(expected) = remote {
                    ensure!(peer == expected, "peer listen address changed");
                }
                ensure!(ch.pin != node.identity.pin(), "cannot pair self");
                {
                    let mut s = state.lock().unwrap();
                    s.pin = ch.pin.clone();
                    s.peer = Some(peer);
                    s.phase = "confirm".into();
                }
                loop {
                    let s = state.lock().unwrap();
                    ensure!(alive(&s), "pairing cancelled or expired");
                    if s.approved {
                        break;
                    }
                    drop(s);
                    std::thread::sleep(Duration::from_millis(100));
                }
                ch.send(1)?;
                ensure!(ch.receive()? == 1, "peer declined pairing");
                ensure!(
                    alive(&state.lock().unwrap()),
                    "pairing cancelled or expired"
                );
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
                    ch.receive()? == 2,
                    "peer did not acknowledge saving; pairing incomplete, retry"
                );
                Ok(())
            };
            let result = run();
            let mut s = state.lock().unwrap();
            s.done = true;
            s.phase = if result.is_ok() { "saved" } else { "failed" }.into();
            if let Err(e) = result {
                s.error = format!("{e}; pairing may be partially saved; inspect config or retry");
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
    Ok(json!({"ok":true,"phase":s.phase,"peer":s.peer,"fingerprint":s.pin,"error":s.error}))
}
