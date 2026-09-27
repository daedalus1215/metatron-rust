use crate::domain::ports::activity_store::ActivityStore;
use crate::domain::ports::clock::Clock;
use crate::domain::StartOutcome;

pub fn start_activity(store: &impl ActivityStore, clock: &impl Clock, name: &str) -> StartOutcome {
    let now = clock.now_ms();
    match store.activity_id(name) {
        Some(id) => { store.open_session(id, now); StartOutcome::Started }
        None => {
            let id = store.create_activity(name);
            store.open_session(id, now);
            StartOutcome::StartedNew
        }
    }
}

pub fn stop_activity(store: &impl ActivityStore, clock: &impl Clock, name: &str) {
    if let Some(id) = store.activity_id(name) {
        store.open_session(id, clock.now_ms());
    }
}
