mod app;
mod infra;
mod ports;

fn main() {
    let mut a = app::App::new(Box::new(infra::Wall));
    a.tick();
    app::cmd_tick(&a, &infra::Sqlite::open());
    infra::Sqlite::open().vacuum();
}
