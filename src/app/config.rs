//! One user-editable config. Only startup loads it into the running Node.
use crate::core::{
    engine::Engine,
    transport::{Identity, Peer, device_from_pin},
};
#[cfg(target_os = "macos")]
use crate::platform::macos::credential;
#[cfg(windows)]
use crate::platform::windows::credential;
use anyhow::{Result, ensure};
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
        json!({"schema":2,"listen":null,"mode":"all","macos_eager_download":false,"peers":[],"identity":null})
    };
    validate(&value)?;
    Ok(value)
}
fn validate(value: &Value) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("config must be an object"))?;
    ensure!(
        object.keys().all(|k| [
            "schema",
            "listen",
            "mode",
            "peers",
            "identity",
            "macos_eager_download"
        ]
        .contains(&k.as_str())),
        "unknown config key"
    );
    ensure!(
        matches!(value["mode"].as_str(), Some("text" | "all")),
        "invalid mode config"
    );
    if !value["listen"].is_null() {
        let _: SocketAddr = value["listen"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("invalid listen"))?
            .parse()?;
    }
    ensure!(
        value["schema"] == 2 && object.contains_key("identity") && value["peers"].is_array(),
        "expected current schema-2 config"
    );
    ensure!(
        value["identity"].is_object()
            || (value["identity"].is_null() && value["peers"].as_array().unwrap().is_empty()),
        "invalid identity"
    );
    if let Some(enabled) = object.get("macos_eager_download") {
        ensure!(enabled.is_boolean(), "macos_eager_download must be boolean");
    }
    peers(value)?;

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
    let temporary = dir.join(format!("config-{}.tmp", crate::core::engine::new_id()));
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
    // Explicit empty current-format identity initializes a fresh installation only.
    // Existing protected credentials are never replaced on a read/decrypt error.
    if value["identity"].is_null() {
        let identity = Identity::generate()?;
        value["identity"] = json!({"certificate_der":identity.cert,"private_key":credential::store(&identity.pin(),identity.private_key_der())?});
        restore(&value)?;
        write(dir, &value)?;
    }
    let identity = restore(&value)?;
    let peers = peers(&value)?;
    ensure!(
        peers.iter().all(|p| p.fingerprint != identity.pin()),
        "cannot pair self"
    );
    let _ = ACTIVE.set(value);
    Ok((identity, peers))
}
pub fn apply(dir: &Path, engine: &mut Engine) -> Result<Option<SocketAddr>> {
    let value = load(dir)?;
    engine.text_only = value["mode"] == "text";
    value["listen"]
        .as_str()
        .map(str::parse)
        .transpose()
        .map_err(Into::into)
}
/// Persist a mutually confirmed peer; active connections change only on restart.
pub fn save_peer(dir: &Path, own_pin: &str, peer: Peer) -> Result<()> {
    let _guard = WRITE_LOCK.lock().unwrap();
    let _file_guard = file_lock(dir)?;
    let mut value = load(dir)?;
    ensure!(peer.fingerprint != own_pin, "cannot pair self");
    let mut next = peers(&value)?;
    next.retain(|p| p.fingerprint != peer.fingerprint);
    next.push(peer);
    value["peers"] = serde_json::to_value(next)?;
    write(dir, &value)
}

pub fn status(node: &crate::core::transport::Node, dir: &Path) -> Result<Value> {
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
    fn protected_identity_persists_and_corruption_fails() {
        let dir = std::env::temp_dir().join(crate::core::engine::new_id());
        std::fs::create_dir(&dir).unwrap();
        let (identity, _) = startup(&dir).unwrap();
        assert_eq!(startup(&dir).unwrap().0.pin(), identity.pin());
        let remote = Identity::generate().unwrap();
        assert!(
            Identity::from_parts(identity.cert.clone(), remote.private_key_der().to_vec()).is_err()
        );
        let peer = Peer {
            address: "127.0.0.1:4567".parse().unwrap(),
            fingerprint: remote.pin(),
        };
        save_peer(&dir, &identity.pin(), peer.clone()).unwrap();
        assert_eq!(startup(&dir).unwrap().1.len(), 1);
        unpair_address(&dir, peer.address).unwrap();
        assert!(startup(&dir).unwrap().1.is_empty());
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
        let mut value =
            json!({"schema":2,"listen":"127.0.0.1:45451","mode":"all","peers":[],"identity":{}});
        validate(&value).unwrap();
        value["macos_eager_download"] = true.into();
        validate(&value).unwrap();
        value["macos_eager_download"] = "true".into();
        assert!(validate(&value).is_err());
        value["macos_eager_download"] = false.into();
        validate(&value).unwrap();
        value["listen"] = "not-an-address".into();
        assert!(validate(&value).is_err());
        value["listen"] = Value::Null;
        value["schema"] = 1.into();
        assert!(validate(&value).is_err());
        value["schema"] = 2.into();
        let peer = json!({"address":"127.0.0.1:45452","fingerprint":"a".repeat(64)});
        value["peers"] = json!([peer.clone(), peer]);
        assert!(validate(&value).is_err());
        value["peers"] = json!([]);
        value["unknown"] = true.into();
        assert!(validate(&value).is_err());
    }
}
