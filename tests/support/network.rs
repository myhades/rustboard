use rustboard::core::transport::Node;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

pub fn sync(source: &Arc<Node>, target: &Arc<Node>) {
    let id = source
        .engine
        .lock()
        .unwrap()
        .current
        .as_ref()
        .unwrap()
        .offer_id
        .clone();
    target.start_sync();
    let deadline = Instant::now() + Duration::from_secs(10);
    while target
        .engine
        .lock()
        .unwrap()
        .current
        .as_ref()
        .map(|offer| &offer.offer_id)
        != Some(&id)
    {
        assert!(Instant::now() < deadline, "offer was not received");
        std::thread::sleep(Duration::from_millis(5));
    }
}
