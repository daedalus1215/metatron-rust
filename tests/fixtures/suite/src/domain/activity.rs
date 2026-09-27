use super::ActivityStore;

pub struct ActivityLog;

impl ActivityStore for ActivityLog {
    fn record(&self, _name: &str) {}
}

pub fn log(store: &dyn ActivityStore) {
    store.record("start");
}
