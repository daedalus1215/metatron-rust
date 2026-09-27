use crate::domain::ports::activity_store::ActivityStore;
use crate::domain::StartOutcome;
use crate::infra::sqlite::SqliteActivityStore;
use std::time::SystemTime;

/// Names the concrete instead of taking `&impl ActivityStore`.
pub fn start_activity(store: &SqliteActivityStore, name: &str) -> StartOutcome {
    let _ = SystemTime::now();
    let _ = std::fs::read_to_string("/etc/hostname");
    stop_activity(store, name);
    StartOutcome::Started
}

/// A use-case calling a use-case: same-level injection.
pub fn stop_activity(store: &SqliteActivityStore, name: &str) {
    let _ = store.activity_id(name);
}

/// naming.md: not an operation name.
pub fn helper(_s: &impl ActivityStore) {}
