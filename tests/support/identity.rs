use rustboard::core::transport::Identity;
use std::io::Write;
pub fn load(dir: &std::path::Path) -> anyhow::Result<Identity> {
    let path = dir.join("identity.json");
    if path.exists() {
        return Ok(serde_json::from_slice(&std::fs::read(path)?)?);
    }
    let identity = Identity::generate()?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(&serde_json::to_vec(&identity)?)?;
    file.sync_all()?;
    Ok(identity)
}
