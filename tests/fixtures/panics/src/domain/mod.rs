//! A domain that unwraps. `panic-in-domain` decides from the model, so the
//! only way for this to pass is for the model to have the calls — and the only
//! way for the rule to stay quiet in a crate whose domain really is clean is
//! for it to say so.

/// Parses a port, believing the input.
pub fn port_of(raw: &str) -> u16 {
    let port: u16 = raw.parse().expect("a port is a number");
    if port == 0 {
        return 80;
    }
    port
}

/// The same, with `unwrap`.
pub fn index_of(items: &[u8], want: u8) -> usize {
    let found = items.iter().position(|i| *i == want);
    found.unwrap()
}
