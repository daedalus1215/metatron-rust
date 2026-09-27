use crate::domain::ports::activity_store::ActivityStore;
use crate::domain::ports::clock::Clock;
use crate::domain::use_cases::activity::start_activity;

/// view-model.md: built by converters, consumed by render fns.
pub struct ActivityRow {
    pub name: String,
    pub minutes: i64,
}

pub fn cmd_start(store: &impl ActivityStore, clock: &impl Clock, name: &str) {
    let _ = start_activity(store, clock, name);
}
