//! One user-editable config. Only startup loads it into the running Node.
#[cfg(target_os = "macos")]
use crate::macos::credential;
#[cfg(windows)]
use crate::windows::credential;
use anyhow::{Result, ensure};
use rustboard_core::{
    engine::Engine,
    transport::{Identity, Peer, device_from_pin},
};
use serde_json::{Value, json};
use std::{io::Write, net::SocketAddr, path::Path, sync::Mutex};
static ACTIVE: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
static WRITE_LOCK: Mutex<()> = Mutex::new(());
fn file_lock(dir: &Path) -> Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join("config.lock"))?;
    file.try_lock()
        .map_err(|e| anyhow::anyhow!("config is busy: {e}"))?;
    Ok(file)
}
fn read_json(path: &Path) -> Result<Value> {
    ensure!(
        std::fs::metadata(path)?.len() <= 131072,
        "config/identity file too large"
    );
    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
}
pub fn load(dir: &Path) -> Result<Value> {
    let path = dir.join("config.json");
    let value = if path.exists() {
        read_json(&path)?
    } else {
        json!({"listen":null,"mode":"all"})
    };
    validate(&value)?;
    Ok(value)
}
fn validate(value: &Value) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("config must be an object"))?;
    ensure!(
        object.keys().all(
            |k| ["schema", "listen", "mode", "paused", "peers", "identity"].contains(&k.as_str())
        ),
        "unknown config key"
    );
    ensure!(
        (value["paused"].is_null() || value["paused"].is_boolean())
            && matches!(value["mode"].as_str(), Some("text" | "all")),
        "invalid mode/paused config"
    );
    if !value["listen"].is_null() {
        let _: SocketAddr = value["listen"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("invalid listen"))?
            .parse()?;
    }
    if !value["schema"].is_null() {
        ensure!(
            value["schema"] == 2 && value["identity"].is_object() && value["peers"].is_array(),
            "invalid config schema/identity/peers"
        );
    }
    if !value["peers"].is_null() {
        peers(value)?;
    }
    Ok(())
}
fn peers(value: &Value) -> Result<Vec<Peer>> {
    let peers: Vec<Peer> = serde_json::from_value(value["peers"].clone())?;
    ensure!(peers.len() <= 16, "peer limit reached");
    let mut seen = std::collections::HashSet::new();
    for peer in &peers {
        device_from_pin(&peer.fingerprint)?;
        ensure!(
            peer.fingerprint == peer.fingerprint.to_lowercase() && seen.insert(&peer.fingerprint),
            "duplicate or noncanonical peer fingerprint"
        );
    }
    Ok(peers)
}
fn write(dir: &Path, value: &Value) -> Result<()> {
    validate(value)?;
    let temporary = dir.join(format!("config-{}.tmp", rustboard_core::engine::new_id()));
    let result = (|| -> Result<()> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(&serde_json::to_vec_pretty(value)?)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, dir.join("config.json"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
fn restore(value: &Value) -> Result<Identity> {
    let cert: Vec<u8> = serde_json::from_value(value["identity"]["certificate_der"].clone())?;
    let key = credential::load(&value["identity"]["private_key"])?;
    Identity::from_parts(cert, key)
}
/// Singleton must be acquired first. Never regenerate on credential store failure.
pub fn startup(dir: &Path) -> Result<(Identity, Vec<Peer>)> {
    let _guard = WRITE_LOCK.lock().unwrap();
    let _file_guard = file_lock(dir)?;
    let mut value = load(dir)?;
    if value["schema"].is_null() {
        ensure!(
            value["identity"].is_null(),
            "legacy config must not contain identity"
        );
        let identity = if dir.join("identity.json").exists() {
            let legacy: Identity = serde_json::from_value(read_json(&dir.join("identity.json"))?)?;
            Identity::from_parts(legacy.cert.clone(), legacy.private_key_der().to_vec())?
        } else {
            Identity::generate()?
        };
        if value["peers"].is_null() {
            value["peers"] = if dir.join("peers.json").exists() {
                read_json(&dir.join("peers.json"))?
            } else {
                json!([])
            };
        }
        peers(&value)?;
        value["identity"] = json!({"certificate_der":identity.cert,"private_key":credential::store(&identity.pin(),identity.private_key_der())?});
        value["schema"] = 2.into();
        let restored = restore(&value)?;
        ensure!(
            restored.pin() == identity.pin(),
            "identity migration mismatch"
        );
        write(dir, &value)?;
    }
    let identity = restore(&value)?;
    let peers = peers(&value)?;
    ensure!(
        peers.iter().all(|p| p.fingerprint != identity.pin()),
        "cannot pair self"
    );
    // Crash after config commit is recoverable. Only remove the verified legacy identity.
    if dir.join("identity.json").exists() {
        let old: Identity = serde_json::from_value(read_json(&dir.join("identity.json"))?)?;
        ensure!(
            old.cert == identity.cert && old.private_key_der() == identity.private_key_der(),
            "legacy identity conflicts with protected config; retained"
        );
        std::fs::remove_file(dir.join("identity.json"))?;
    }
    if dir.join("peers.json").exists() {
        std::fs::remove_file(dir.join("peers.json"))?;
    }
    if value.as_object_mut().unwrap().remove("paused").is_some() {
        write(dir, &value)?;
    }
    let _ = ACTIVE.set(value);
    Ok((identity, peers))
}
pub fn apply(dir: &Path, engine: &mut Engine) -> Result<Option<SocketAddr>> {
    let value = load(dir)?;
    engine.paused = false;
    engine.text_only = value["mode"] == "text";
    value["listen"]
        .as_str()
        .map(str::parse)
        .transpose()
        .map_err(Into::into)
}
/// CLI writes desired config only; running peers/mode remain unchanged until restart.
pub fn update(dir: &Path, own_pin: &str, args: &[String]) -> Result<Value> {
    let _guard = WRITE_LOCK.lock().unwrap();
    let _file_guard = file_lock(dir)?;
    let mut value = load(dir)?;
    ensure!(
        value["schema"] == 2,
        "restart Agent to migrate config first"
    );
    match args[0].as_str() {
        "mode" => {
            ensure!(
                args.len() == 2 && ["text", "all"].contains(&args[1].as_str()),
                "mode text|all"
            );
            value["mode"] = args[1].clone().into();
        }
        "pair" => {
            ensure!(
                args.len() == 4 && args[2] == "--fingerprint",
                "pair <IP:port> --fingerprint <SHA256>; restart BOTH Agents"
            );
            let pin = args[3].to_lowercase();
            device_from_pin(&pin)?;
            ensure!(pin != own_pin, "cannot pair self");
            let mut next = peers(&value)?;
            next.retain(|p| p.fingerprint != pin);
            next.push(Peer {
                address: args[1].parse()?,
                fingerprint: pin,
            });
            value["peers"] = serde_json::to_value(next)?;
        }
        "unpair" => {
            ensure!(args.len() == 2, "unpair <device_id or fingerprint>");
            let mut next = peers(&value)?;
            next.retain(|p| p.device() != args[1] && p.fingerprint != args[1]);
            value["peers"] = serde_json::to_value(next)?;
        }
        _ => anyhow::bail!("unknown config command"),
    }
    write(dir, &value)?;
    Ok(
        json!({"ok":true,"restart_required":true,"message":"Configuration saved; restart Agent to apply. Existing connections remain active until restart."}),
    )
}

pub fn status(node: &rustboard_core::transport::Node, dir: &Path) -> Result<Value> {
    let desired = load(dir)?;
    let active = ACTIVE.get();
    Ok(
        json!({"ok":true,"version":env!("CARGO_PKG_VERSION"),"fingerprint":node.identity.pin(),"device_id":node.identity.device(),"mode":if node.engine.lock().unwrap().text_only{"text"}else{"all"},"listen":active.map(|v| &v["listen"]),"peers":*node.peers.lock().unwrap(),"pending_restart":active.is_some_and(|v|v!=&desired)}),
    )
}
pub fn unpair_address(dir: &Path, address: std::net::SocketAddr) -> Result<()> {
    let _guard = WRITE_LOCK.lock().unwrap();
    let _file_guard = file_lock(dir)?;
    let mut value = load(dir)?;
    let mut next = peers(&value)?;
    ensure!(
        next.iter().filter(|p| p.address == address).count() == 1,
        "address absent or ambiguous; inspect status/config"
    );
    next.retain(|p| p.address != address);
    value["peers"] = serde_json::to_value(next)?;
    write(dir, &value)
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn migration_preserves_identity_and_restart_applies_peer_edits() {
        let dir = std::env::temp_dir().join(rustboard_core::engine::new_id());
        std::fs::create_dir(&dir).unwrap();
        let old = Identity::generate().unwrap();
        std::fs::write(dir.join("identity.json"), serde_json::to_vec(&old).unwrap()).unwrap();
        let remote = Identity::generate().unwrap();
        assert!(Identity::from_parts(old.cert.clone(), remote.private_key_der().to_vec()).is_err());
        let peer = Peer {
            address: "127.0.0.1:4567".parse().unwrap(),
            fingerprint: remote.pin(),
        };
        std::fs::write(
            dir.join("peers.json"),
            serde_json::to_vec(&vec![peer]).unwrap(),
        )
        .unwrap();
        let (identity, active) = startup(&dir).unwrap();
        assert_eq!(old.pin(), identity.pin());
        assert_eq!(active.len(), 1);
        assert!(!dir.join("identity.json").exists() && !dir.join("peers.json").exists());
        // Simulate a crash after config commit but before legacy cleanup.
        std::fs::write(dir.join("identity.json"), serde_json::to_vec(&old).unwrap()).unwrap();
        std::fs::write(dir.join("peers.json"), b"[]").unwrap();
        assert_eq!(startup(&dir).unwrap().1.len(), 1);
        assert!(!dir.join("identity.json").exists());
        assert!(!load(&dir).unwrap()["identity"]["private_key"]["blob"].is_null());
        update(&dir, &identity.pin(), &["unpair".into(), remote.pin()]).unwrap();
        assert_eq!(active.len(), 1);
        assert!(startup(&dir).unwrap().1.is_empty());
        update(&dir, &identity.pin(), &["mode".into(), "text".into()]).unwrap();
        let mut engine = Engine::new(identity.device());
        apply(&dir, &mut engine).unwrap();
        assert!(engine.text_only);
        assert!(!engine.paused);
        let mut broken = load(&dir).unwrap();
        broken["identity"]["private_key"]["blob"] = json!([1, 2, 3]);
        write(&dir, &broken).unwrap();
        assert!(startup(&dir).is_err());
        assert_eq!(load(&dir).unwrap(), broken);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod validation_tests {
    use super::*;
    #[test]
    fn malformed_settings_and_duplicate_peers_are_rejected() {
        let mut value = json!({"listen":"127.0.0.1:45451","mode":"all","paused":false,"peers":[]});
        validate(&value).unwrap();
        value["listen"] = "not-an-address".into();
        assert!(validate(&value).is_err());
        value["listen"] = Value::Null;
        value["schema"] = 2.into();
        assert!(validate(&value).is_err());
        value["identity"] = json!({});
        let peer = json!({"address":"127.0.0.1:45452","fingerprint":"a".repeat(64)});
        value["peers"] = json!([peer.clone(), peer]);
        assert!(validate(&value).is_err());
        value["peers"] = json!([]);
        value["unknown"] = true.into();
        assert!(validate(&value).is_err());
    }
}
