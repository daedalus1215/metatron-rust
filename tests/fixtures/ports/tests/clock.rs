// An integration test target, so the conforming fixture has a suite for the
// test-presence rule to have a premise about. It calls the clock port and the
// note store, and leaves the activity store alone — so the rule has both an
// exercised and an unexercised port to say something about.
use ports::domain::ports::clock::Clock;
use ports::domain::ports::note_store::NoteStore;

struct Frozen;

impl Clock for Frozen {
    fn now_ms(&self) -> i64 {
        0
    }
}

struct Notes;

impl NoteStore for Notes {
    fn save(&self, _note: &str) {}
    fn all(&self) -> Vec<String> {
        Vec::new()
    }
}

#[test]
fn the_clock_can_be_frozen() {
    assert_eq!(Frozen.now_ms(), 0);
}

#[test]
fn notes_start_empty() {
    assert!(Notes.all().is_empty());
}
