mod application;
mod domain;
mod infra;

fn main() {
    // Composition root: the only place a concrete is named.
    let store = infra::sqlite::SqliteActivityStore::open();
    let clock = infra::mem::FixedClock;
    application::cli::cmd_start(&store, &clock, "coding");
}
