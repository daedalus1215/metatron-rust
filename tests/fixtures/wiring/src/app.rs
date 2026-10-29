use crate::ports::{Clock, Store};

pub struct App {
    clock: Box<dyn Clock>,
    ledger: Ledger,
    ghost: Phantom,
}

pub struct Ledger {
    entries: Vec<u64>,
}

impl Ledger {
    pub fn record(&mut self, at: u64) {
        self.entries.push(at);
    }

    pub fn total(&self) -> usize {
        self.entries.len()
    }

    pub fn clear(&mut self) {}
}

impl App {
    pub fn new(clock: Box<dyn Clock>) -> Self {
        unimplemented!()
    }

    pub fn tick(&mut self) {
        let at = self.clock.now();
        self.ledger.record(at);
        self.helper();
    }

    fn helper(&self) {}

    pub fn idle(&self) {}

    pub fn ledger(&self) -> Ledger {
        unimplemented!()
    }

    pub fn unused(&self) {}
}

pub fn cmd_tick<S: Store>(app: &App, store: &S) {
    let x = store.load();
    store.save(x);
    let built = Ledger { entries: vec![] };
    let _ = built.total();
}

pub fn cmd_idle(app: &App) {
    app.idle();
    // A method's result: its type would have to be inferred.
    let mut found = app.ledger();
    found.clear();
}
