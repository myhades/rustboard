//! Shared CLI presentation; service integration stays in platform modules.
#[cfg(target_os = "macos")]
use crate::platform::macos::{ipc, service};
#[cfg(windows)]
use crate::platform::windows::{ipc, service};
use anyhow::{Result, ensure};
use serde_json::Value;
use std::{
    io::{self, Write},
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    time::Duration,
};
pub fn request(args: &[&str]) -> Result<Value> {
    let v = ipc::request(args.iter().map(|s| s.to_string()).collect())?;
    ensure!(
        v["ok"] == true,
        "{}",
        v["error"].as_str().unwrap_or("Agent command failed")
    );
    Ok(v)
}
pub fn address(s: &str) -> Result<SocketAddr> {
    let a = if let Ok(ip) = s.parse::<IpAddr>() {
        SocketAddr::new(ip, 45452)
    } else {
        s.parse()?
    };
    ensure!(a.port() > 0 && a.port() < 65535, "port must be 1..65534");
    Ok(a)
}
fn group(pin: &str) -> String {
    pin.as_bytes()
        .chunks(4)
        .map(|b| String::from_utf8_lossy(b).into_owned())
        .collect::<Vec<_>>()
        .join(" ")
}
fn config_path() -> Result<PathBuf> {
    Ok(ipc::data_dir()?.join("config.json"))
}
pub fn run(args: Vec<String>) -> Result<()> {
    let cmd = args.first().map(String::as_str).unwrap_or("help");
    if cmd == "help" {
        ensure!(args.len() <= 2, "usage: rustboard help [COMMAND]");
        return help(args.get(1).map(String::as_str));
    }
    if cmd == "version" {
        ensure!(args.len() == 1, "usage: rustboard version");
        println!("rustboard {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if cmd == "status" && args.len() == 2 && args[1] == "--json" {
        println!("{}", serde_json::to_string_pretty(&request(&["status"])?)?);
        return Ok(());
    }
    ensure!(
        matches!(
            cmd,
            "status" | "start" | "stop" | "restart" | "config" | "pair" | "unpair"
        ),
        "unknown command '{cmd}'; run 'rustboard help'"
    );
    ensure!(
        args.len() == 1 || (matches!(cmd, "pair" | "unpair") && args.len() == 2),
        "unexpected arguments; run 'rustboard help {cmd}'"
    );
    match cmd {
        "status" => {
            let running = request(&["status"]);
            println!(
                "  Service    {}",
                if running.is_ok() {
                    "running"
                } else {
                    "stopped"
                }
            );
            if let Ok(v) = running {
                println!(
                    "  Listen     {}\n  Mode       {}",
                    v["listen"].as_str().unwrap_or("off"),
                    v["mode"].as_str().unwrap_or("unknown")
                );
                print_peers(&v["peers"]);
            }
        }
        "start" => {
            service::start(true)?;
        }
        "stop" => {
            service::stop(true)?;
        }
        "restart" => {
            ensure!(
                service::enabled()?,
                "service is disabled; use start to enable it"
            );
            service::stop(false)?;
            service::start(false)?;
        }
        "config" => {
            let p = config_path()?;
            ensure!(
                p.is_file(),
                "configuration not found: {}; run 'rustboard start' first",
                p.display()
            );
            service::edit(&p)?;
            println!("{}", p.display());
        }
        "unpair" => {
            ensure!(args.len() == 2, "usage: rustboard unpair IP");
            let a: IpAddr = args[1].parse().map_err(|_| {
                anyhow::anyhow!(
                    "expected an IP address without a port; run 'rustboard help unpair'"
                )
            })?;
            if request(&["status"]).is_ok() {
                request(&["unpair-ip", &a.to_string()])?;
            } else {
                crate::app::config::unpair_ip(&ipc::data_dir()?, a)?;
            }
            println!("Removed {a}. Restart to revoke existing trust.");
        }
        "pair" => pair(args.get(1).map(String::as_str))?,
        _ => unreachable!(),
    }
    Ok(())
}
fn help(command: Option<&str>) -> Result<()> {
    let Some(command) = command else {
        println!(
            "\
Synchronize the clipboard between paired computers.

Usage: rustboard COMMAND [ARGUMENTS]

Commands:
  status [--json]     Show service state and peers
  start               Start the service and enable login startup
  stop                Stop the service and disable login startup
  restart             Restart the service and apply saved configuration
  config              Open the configuration in your text editor
  pair [IP[:PORT]]    Wait for pairing or connect to a peer (default port: 45452)
  unpair IP           Remove pairing for an IP address
  help [COMMAND]      Show help for all commands or a specific command
  version             Show the installed CLI version

Run 'rustboard help COMMAND' for command details."
        );
        return Ok(());
    };
    let detail = match command {
        "status" => {
            "status [--json]\n\nShow service state, listen address, mode and peers.\nPeer entries show saved trust, not connection status.\nWith --json, print the Agent response; fail if the Agent is unavailable."
        }
        "start" => "start\n\nStart the service now and enable automatic startup at login.",
        "stop" => {
            "stop\n\nStop the service now and disable automatic startup at login.\nKeep configuration, device identity and peers."
        }
        "restart" => {
            "restart\n\nRestart the service to apply configuration and pairing changes.\nActive transfers are interrupted. A disabled service requires 'rustboard start'."
        }
        "config" => {
            "config\n\nOpen the existing configuration using VISUAL, then EDITOR, or the system\ntext editor. VISUAL and EDITOR must name an executable.\nSave your changes, then run 'rustboard restart' to apply them."
        }
        "pair" => {
            "pair [IP[:PORT]]\n\nWithout an address, wait for an incoming pairing request. With an address,\nconnect to that peer; the default port is 45452. Use [IPv6]:PORT for IPv6\nwith an explicit port. Compare fingerprints on both peers before accepting.\nBoth services must be running. Each service restarts after pairing succeeds."
        }
        "unpair" => {
            "unpair IP\n\nRemove all saved pairings at the specified IPv4 or IPv6 address. Do not\ninclude a port. Restart the service to revoke existing trust.\nTo revoke access immediately, stop the service before removing the pairing."
        }
        "help" => "help [COMMAND]\n\nShow the command list or help for one command.",
        "version" => "version\n\nShow the version of this CLI executable.",
        _ => anyhow::bail!("unknown command '{command}'; run 'rustboard help'"),
    };
    println!("Usage: rustboard {detail}");
    Ok(())
}
fn print_peers(peers: &Value) {
    let peers = peers.as_array();
    println!("\nPeers ({})", peers.map_or(0, Vec::len));
    if let Some(peers) = peers.filter(|p| !p.is_empty()) {
        for p in peers {
            println!("  {}", p["address"].as_str().unwrap_or("?"));
        }
    }
}
fn pair(target: Option<&str>) -> Result<()> {
    ensure!(request(&["status"]).is_ok(), "not running");
    let target = target.map(address).transpose()?.map(|a| a.to_string());
    let interrupt = service::PairInterrupt::install()?;
    let v = if let Some(a) = &target {
        request(&["pair-begin", a])?
    } else {
        request(&["pair-begin"])?
    };
    let id = v["session"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("missing session"))?
        .to_string();
    struct Cancel(String);
    impl Drop for Cancel {
        fn drop(&mut self) {
            if request(&["pair-cancel", &self.0]).is_err() {
                return;
            }
            // Wait until the worker releases its listener/socket before another pair can begin.
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while std::time::Instant::now() < deadline {
                match request(&["pair-poll", &self.0]) {
                    Ok(value) if matches!(value["phase"].as_str(), Some("failed" | "saved")) => {
                        break;
                    }
                    Err(_) => break,
                    _ => std::thread::sleep(Duration::from_millis(20)),
                }
            }
        }
    }
    let _guard = Cancel(id.clone());
    println!(
        "Local fingerprint:\n  {}\nWaiting...",
        group(v["fingerprint"].as_str().unwrap_or(""))
    );
    let (tx, rx) = std::sync::mpsc::channel();
    let mut prompted = false;
    loop {
        ensure!(!interrupt.interrupted(), "pairing cancelled");
        let v = request(&["pair-poll", &id])?;
        match v["phase"].as_str().unwrap_or("") {
            "confirm" if !prompted => {
                println!(
                    "Peer {}\n  {}",
                    v["peer"].as_str().unwrap_or("?"),
                    group(v["fingerprint"].as_str().unwrap_or(""))
                );
                let peer: SocketAddr = v["peer"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("missing peer address"))?
                    .parse()?;
                let config = crate::app::config::load(&ipc::data_dir()?)?;
                if config["peers"].as_array().is_some_and(|peers| {
                    peers.iter().any(|p| {
                        p["address"]
                            .as_str()
                            .and_then(|a| a.parse::<SocketAddr>().ok())
                            == Some(peer)
                    })
                }) {
                    println!(
                        "rustboard: peer \"{}\" already exists and will be updated",
                        peer.ip()
                    );
                }
                print!("Compare fingerprints on both peers. Accept? [y/N] ");
                io::stdout().flush()?;
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let ok = accept_pairing(&mut io::stdin().lock());
                    let _ = tx.send(ok);
                });
                prompted = true;
            }
            "saved" => {
                println!("Peer added. Restarting service...");
                drop(_guard);
                service::stop(false).and_then(|_| service::start(false))?;
                return Ok(());
            }
            "failed" => anyhow::bail!("{}", v["error"].as_str().unwrap_or("pairing failed")),
            _ => {}
        }
        if let Ok(ok) = rx.try_recv() {
            ensure!(!interrupt.interrupted(), "pairing cancelled");
            ensure!(ok, "pairing declined");
            request(&["pair-approve", &id])?;
            println!("Confirm on the other peer...");
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

fn accept_pairing(reader: &mut impl io::BufRead) -> bool {
    let mut line = String::new();
    if !matches!(reader.read_line(&mut line), Ok(n) if n > 0) {
        return false;
    }
    let answer = line.trim();
    answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes")
}

#[cfg(test)]
mod tests {
    use super::accept_pairing;
    #[test]
    fn pairing_requires_explicit_approval() {
        for input in ["y\n", "YES\n"] {
            assert!(accept_pairing(&mut input.as_bytes()));
        }
        for input in ["", "\n", "\r\n", "n\n", "no\n", "invalid\n"] {
            assert!(!accept_pairing(&mut input.as_bytes()));
        }
    }
}
