//! `tally` is assigned and never consulted, here or anywhere else in the
//! crate. `total` is assigned and read, so it is not a finding.

pub struct Counter {
    tally: u32,
    total: u32,
    a: u32,
    b: u32,
    c: u32,
}

impl Counter {
    pub fn bump(&mut self) {
        self.tally = self.a + 1;
        self.total = self.b + 1;
    }
    pub fn report(&self) -> u32 {
        self.total + self.c
    }
    pub fn reset(&mut self) {
        self.tally = 0;
    }
}
