use crate::domain::{log, ActivityLog, ActivityStore};

pub fn start(store: &dyn ActivityStore) {
    log(store);
    store.record("start");
}

pub fn default_store() -> ActivityLog {
    ActivityLog
}
