use crate::domain::ports::activity_store::ActivityStore;
use crate::domain::{Activity, StartOutcome};

pub struct SqliteActivityStore { conn: rusqlite::Connection }

impl SqliteActivityStore {
    pub fn open() -> Self { unimplemented!() }

    /// store.md: the store is making a business decision.
    pub fn start(&self, name: &str) -> StartOutcome {
        match self.activity_id(name) {
            Some(_) => StartOutcome::AlreadyRunning,
            None => StartOutcome::StartedNew,
        }
    }
}

impl ActivityStore for SqliteActivityStore {
    fn activity_id(&self, _name: &str) -> Option<i64> { None }
    fn create_activity(&self, _name: &str) -> i64 { 0 }
    fn open_session(&self, _id: i64, _now: i64) {}
    fn get(&self, _id: i64) -> Option<Activity> { None }
    fn raw(&self) -> &rusqlite::Connection { &self.conn }
}
