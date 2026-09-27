use crate::domain::ports::activity_store::ActivityStore;
use crate::domain::ports::clock::Clock;
use crate::domain::Activity;

/// port.md: "Fake impl: co-located with the code that tests it, or
/// `infra/mem.rs`." It implements a port and names no external crate at
/// all, so neither the store nor the adapter extern set decides it.
pub struct MemStore;

impl ActivityStore for MemStore {
    fn activity_id(&self, _name: &str) -> Option<i64> { None }
    fn create_activity(&self, _name: &str) -> i64 { 0 }
    fn open_session(&self, _id: i64, _now: i64) {}
    fn get(&self, _id: i64) -> Option<Activity> { None }
}

pub struct FixedClock;

impl Clock for FixedClock {
    fn now_ms(&self) -> i64 { 0 }
}

pub struct MemNotes;

impl crate::domain::ports::note_store::NoteStore for MemNotes {
    fn read_note(&self, _path: &str) -> String { String::new() }
}
