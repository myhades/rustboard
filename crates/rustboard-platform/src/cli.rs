//! Shared CLI presentation; service integration stays in platform modules.
#[cfg(target_os = "macos")]
use crate::macos::{ipc, service};
#[cfg(windows)]
use crate::windows::{ipc, service};
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
    let cmd = args.first().map(String::as_str).unwrap_or("--help");
    if cmd == "--help" {
        println!(
            "Rustboard {}\n\n  status                 Show service and paired devices\n  start                  Start now and enable login startup\n  stop                   Stop now and disable login startup\n  restart                Reload config (preserves startup setting)\n  config                 Open config in your text editor\n  pair [IP[:port]]        Wait for pairing, or connect and confirm\n  unpair IP[:port]        Remove a peer; restart to apply\n\n  --help  --version\n\nDefault port: 45452. Pairing uses the next TCP port temporarily.",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    if cmd == "--version" {
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
        "unknown command; use --help"
    );
    ensure!(
        args.len() == 1 || (matches!(cmd, "pair" | "unpair") && args.len() == 2),
        "invalid arguments; use --help"
    );
    match cmd {
        "status" => {
            let enabled = service::enabled()?;
            let running = request(&["status"]);
            println!(
                "Rustboard {}\n  Service    {}\n  Auto-start {}",
                env!("CARGO_PKG_VERSION"),
                if running.is_ok() {
                    "running"
                } else {
                    "unavailable"
                },
                if enabled { "enabled" } else { "disabled" }
            );
            if let Ok(v) = running {
                println!(
                    "  Listen     {}\n  Mode       {}\n  Config     {}",
                    v["listen"].as_str().unwrap_or("off"),
                    v["mode"].as_str().unwrap_or("unknown"),
                    if v["pending_restart"] == true {
                        "restart required"
                    } else {
                        "loaded"
                    }
                );
                print_peers(&v["peers"]);
            } else {
                if let Ok(c) = crate::config::load(&ipc::data_dir()?) {
                    print_peers(&c["peers"]);
                }
                println!(
                    "  Agent is stopped or unreachable; inspect logs if startup was expected."
                );
            }
        }
        "start" => {
            service::start(true)?;
            println!("Started. Login startup enabled.");
        }
        "stop" => {
            service::stop(true)?;
            println!("Stopped. Login startup disabled.");
        }
        "restart" => {
            ensure!(
                service::enabled()?,
                "service is disabled; use start to enable it"
            );
            service::stop(false)?;
            service::start(false)?;
            println!("Restarted; configuration loaded.");
        }
        "config" => {
            let p = config_path()?;
            ensure!(p.exists(), "install and initialize Rustboard first");
            service::edit(&p)?;
            println!(
                "Config: {}\nRun rustboard restart after saving.",
                p.display()
            );
        }
        "unpair" => {
            ensure!(args.len() == 2, "unpair requires IP[:port]");
            let a = address(&args[1])?;
            if request(&["status"]).is_ok() {
                request(&["unpair-address", &a.to_string()])?;
            } else {
                crate::config::unpair_address(&ipc::data_dir()?, a)?;
            }
            println!("Removed {a}. Restart to revoke existing trust.");
        }
        "pair" => pair(args.get(1).map(String::as_str))?,
        _ => unreachable!(),
    }
    Ok(())
}
fn print_peers(peers: &Value) {
    let peers = peers.as_array();
    println!("\n  Paired devices ({})", peers.map_or(0, Vec::len));
    if let Some(peers) = peers {
        for p in peers {
            let pin = p["fingerprint"].as_str().unwrap_or("");
            println!(
                "    {:<24} {}",
                p["address"].as_str().unwrap_or("?"),
                &pin[..pin.len().min(12)]
            );
        }
    }
}
fn pair(target: Option<&str>) -> Result<()> {
    let target = target.map(address).transpose()?.map(|a| a.to_string());
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
            let _ = request(&["pair-cancel", &self.0]);
        }
    }
    let _guard = Cancel(id.clone());
    println!(
        "Local fingerprint:\n  {}\nWaiting for pairing (120 seconds).",
        group(v["fingerprint"].as_str().unwrap_or(""))
    );
    let (tx, rx) = std::sync::mpsc::channel();
    let mut prompted = false;
    loop {
        let v = request(&["pair-poll", &id])?;
        match v["phase"].as_str().unwrap_or("") {
            "confirm" if !prompted => {
                println!(
                    "Peer {}\n  {}",
                    v["peer"].as_str().unwrap_or("?"),
                    group(v["fingerprint"].as_str().unwrap_or(""))
                );
                print!("Compare fingerprints on BOTH devices. Accept? [y/N] ");
                io::stdout().flush()?;
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let mut line = String::new();
                    let ok = io::stdin().read_line(&mut line).is_ok()
                        && matches!(line.trim(), "y" | "Y" | "yes");
                    let _ = tx.send(ok);
                });
                prompted = true;
            }
            "saved" => {
                println!("Pairing saved on both devices. Restart both services to apply.");
                return Ok(());
            }
            "failed" => anyhow::bail!("{}", v["error"].as_str().unwrap_or("pairing failed")),
            _ => {}
        }
        if let Ok(ok) = rx.try_recv() {
            ensure!(ok, "pairing declined");
            request(&["pair-approve", &id])?;
            println!("Waiting for peer confirmation and save...");
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}
