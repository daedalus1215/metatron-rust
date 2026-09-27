use crate::domain::ports::activity_store::ActivityStore;
use crate::domain::Activity;

pub struct SqliteActivityStore { conn: rusqlite::Connection }

impl SqliteActivityStore {
    pub fn open() -> Self { unimplemented!() }
}

impl ActivityStore for SqliteActivityStore {
    fn activity_id(&self, _name: &str) -> Option<i64> { None }
    fn create_activity(&self, _name: &str) -> i64 { 0 }
    fn open_session(&self, _id: i64, _now: i64) {}
    fn get(&self, _id: i64) -> Option<Activity> { None }
}
