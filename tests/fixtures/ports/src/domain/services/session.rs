use crate::domain::ports::activity_store::ActivityStore;
use crate::domain::ports::clock::Clock;
use crate::domain::use_cases::activity::{start_activity, stop_activity};
use crate::domain::StartOutcome;

/// service.md: orchestrates use-cases. Two of them, so it is a workflow
/// rather than ceremony.
pub fn switch_activity(
    store: &impl ActivityStore,
    clock: &impl Clock,
    from: &str,
    to: &str,
) -> StartOutcome {
    stop_activity(store, clock, from);
    start_activity(store, clock, to)
}
