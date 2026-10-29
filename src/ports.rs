//! Ports: the traits that are the only way the rest of the crate talks to
//! the outside world (patterns-rust `domain/port.md`). One trait per
//! concern; the real impls live in their own infrastructure modules and
//! are bound at the composition root (`main.rs`).

/// The only way the rest of the crate asks "what time is it?".
///
/// Time is a port so tests are deterministic (naming.md: ports are nouns
/// for the capability — `ActivityStore`, `Clock`, `Filesystem`).
pub trait Clock {
    /// Seconds since the Unix epoch.
    fn now_secs(&self) -> u64;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clock stuck in time — this module's `MemStore`, co-located with
    /// the code that tests it (testing.md: one fake per port).
    struct FixedClock {
        secs: u64,
    }

    impl Clock for FixedClock {
        fn now_secs(&self) -> u64 {
            self.secs
        }
    }

    #[test]
    fn fixed_clock_says_what_it_was_told() {
        let c = FixedClock { secs: 1_761_234_567 };
        assert_eq!(c.now_secs(), 1_761_234_567);
        assert_eq!(c.now_secs(), 1_761_234_567, "a fixed clock does not drift");
    }

    #[test]
    fn system_clock_is_plausible() {
        // Not a determinism test — the fixed clock covers that. This one
        // only proves the adapter talks to the OS at all.
        let s = crate::clock::SystemClock.now_secs();
        assert!(s > 1_700_000_000, "implausible epoch seconds: {s}");
    }
}
