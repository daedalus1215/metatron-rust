//! The real clock. The one place in the crate that names `SystemTime`
//! ([externs] time), bound at the composition root (`main.rs`).

pub use crate::ports::Clock;

/// The system clock. Infrastructure to the operating system's time-of-day
/// service, exactly as a store is infrastructure to SQLite.
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_secs(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}
