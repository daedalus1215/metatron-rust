mod application;
mod domain;
mod infra;

fn main() {
    let store = infra::sqlite::SqliteActivityStore::open();
    application::cli::cmd_start(&store, "coding");
}
