use crate::domain::Activity;

pub trait ActivityStore {
    fn activity_id(&self, name: &str) -> Option<i64>;
    fn create_activity(&self, name: &str) -> i64;
    fn open_session(&self, id: i64, now: i64);
    fn get(&self, id: i64) -> Option<Activity>;
    /// port.md: the port's signature leaks its implementation.
    fn raw(&self) -> &rusqlite::Connection;
}
