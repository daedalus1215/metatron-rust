//! One module, three layers — the shape spec 02's detector 2 exists to
//! find, and the shape `arioch/src/app.rs` actually has.

/// domain: a value type with no I/O.
pub enum Mode {
    Normal,
    Search,
}

/// application: an entry point.
pub fn handle_key(_k: u8) -> Mode {
    Mode::Normal
}

/// infrastructure: reads the disk.
pub fn load_config(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}
