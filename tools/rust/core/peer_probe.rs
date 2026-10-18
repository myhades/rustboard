//! Explicit diagnostic: use an existing test profile's pins to read one block.
#[path = "../../../tests/support/identity.rs"]
mod test_identity;
use anyhow::{Context, Result};
use rustboard_core::{
    Content, ReadAt,
    transport::{Node, Peer, RemoteSource},
};
fn main() -> Result<()> {
    let dir = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .context("test profile directory required")?,
    );
    anyhow::ensure!(
        dir.join("identity.json").is_file(),
        "existing identity required"
    );
    let peers: Vec<Peer> = serde_json::from_slice(&std::fs::read(dir.join("peers.json"))?)?;
    anyhow::ensure!(peers.len() == 1, "exactly one test peer required");
    let node = Node::new(test_identity::load(&dir)?, peers.clone());
    node.sync_once(&peers[0])?;
    let offer = node
        .engine
        .lock()
        .unwrap()
        .current
        .clone()
        .context("no offer")?;
    let Content::Files { files } = &offer.content else {
        anyhow::bail!("not files")
    };
    for file in files {
        let source = RemoteSource::new(node.clone(), peers[0].clone(), offer.clone(), file.clone());
        let mut bytes = vec![0; 4096];
        let n = source.read_at(0, &mut bytes)?;
        anyhow::ensure!(
            bytes[..n]
                .iter()
                .enumerate()
                .all(|(i, b)| *b == (i % 251) as u8),
            "pattern mismatch"
        );
        println!("size={} first_block={} validated", source.len(), n);
    }
    Ok(())
}
