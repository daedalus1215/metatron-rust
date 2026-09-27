use crate::domain::StartOutcome;
use crate::infra::sqlite::SqliteActivityStore;

pub static mut CALL_COUNT: u64 = 0;

pub struct ActivityRow { pub name: String, pub store: SqliteActivityStore }

/// command-handler.md: the handler is branching on domain state.
pub fn cmd_start(store: &SqliteActivityStore, name: &str) {
    match store.start(name) {
        StartOutcome::AlreadyRunning => {}
        StartOutcome::Started => {}
        StartOutcome::StartedNew => {}
    }
}

/// view-model.md: the render fn reaches through the object graph.
pub fn render_row(row: &ActivityRow) -> String {
    format!("{}", row.store.conn.path)
}
