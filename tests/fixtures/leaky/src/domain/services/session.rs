use crate::domain::use_cases::activity::start_activity;
use crate::infra::sqlite::SqliteActivityStore;
use crate::domain::StartOutcome;

/// service.md: wraps exactly one use-case — ceremony without a workflow.
pub fn begin(store: &SqliteActivityStore, name: &str) -> StartOutcome {
    start_activity(store, name)
}
