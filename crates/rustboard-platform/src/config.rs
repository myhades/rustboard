//! Small per-profile deployment settings, shared by the native adapters.
use anyhow::{Result, ensure};
use rustboard_core::engine::Engine;
use std::{net::SocketAddr, path::Path};

pub fn load(dir: &Path) -> Result<serde_json::Value> {
    let path = dir.join("config.json");
    let value: serde_json::Value = if path.exists() {
        ensure!(std::fs::metadata(&path)?.len() <= 16384, "config too large");
        serde_json::from_slice(&std::fs::read(path)?)?
    } else {
        serde_json::json!({"listen":null,"mode":"all","paused":false})
    };
    let object = value
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("config must be an object"))?;
    ensure!(
        object
            .keys()
            .all(|k| ["listen", "mode", "paused"].contains(&k.as_str())),
        "unknown config key"
    );
    ensure!(
        value["paused"].is_boolean() && matches!(value["mode"].as_str(), Some("text" | "all")),
        "invalid mode/paused config"
    );
    if !value["listen"].is_null() {
        let _: SocketAddr = value["listen"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("invalid listen"))?
            .parse()?;
    }
    Ok(value)
}
pub fn apply(dir: &Path, engine: &mut Engine) -> Result<Option<SocketAddr>> {
    let value = load(dir)?;
    engine.paused = value["paused"].as_bool().unwrap();
    engine.text_only = value["mode"] == "text";
    value["listen"]
        .as_str()
        .map(str::parse)
        .transpose()
        .map_err(Into::into)
}
pub fn save_state(dir: &Path, engine: &Engine) -> Result<()> {
    let mut value = load(dir)?;
    value["paused"] = engine.paused.into();
    value["mode"] = if engine.text_only { "text" } else { "all" }.into();
    let temporary = dir.join("config.new");
    std::fs::write(&temporary, serde_json::to_vec_pretty(&value)?)?;
    std::fs::rename(temporary, dir.join("config.json"))?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_survive_restart_and_reject_invalid_values() {
        let dir = std::env::temp_dir().join(rustboard_core::engine::new_id());
        std::fs::create_dir(&dir).unwrap();
        let mut engine = Engine::new(rustboard_core::engine::new_id());
        assert!(apply(&dir, &mut engine).unwrap().is_none());
        std::fs::write(
            dir.join("config.json"),
            r#"{"listen":"127.0.0.1:45451","mode":"all","paused":false}"#,
        )
        .unwrap();
        engine.paused = true;
        engine.text_only = true;
        save_state(&dir, &engine).unwrap();
        let mut restarted = Engine::new(rustboard_core::engine::new_id());
        assert_eq!(apply(&dir, &mut restarted).unwrap().unwrap().port(), 45451);
        assert!(restarted.paused && restarted.text_only);
        std::fs::write(dir.join("config.json"), r#"{"mode":"bad","paused":false}"#).unwrap();
        assert!(load(&dir).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
