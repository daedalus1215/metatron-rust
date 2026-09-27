use crate::domain::ports::note_store::NoteStore;
use std::fs;

/// adapter.md: "the filesystem, an HTTP API, the clipboard, spawning
/// $EDITOR". Distinguished from a store by what it talks to, not by shape.
pub struct RealFs;

impl NoteStore for RealFs {
    fn read_note(&self, path: &str) -> String {
        fs::read_to_string(path).unwrap_or_default()
    }
}
