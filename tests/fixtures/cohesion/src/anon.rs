//! `Split` with every name destroyed and the access structure preserved.
//! The partition must come out identical, or the detector is a naming lint.

pub struct Anon {
    f1: u32,
    f2: u32,
    f3: u32,
    f4: u32,
    f5: u32,
    f6: u32,
}

impl Anon {
    pub fn m1(&self) -> u32 {
        self.f1 + self.f2
    }
    pub fn m2(&mut self) {
        self.f3 += 1;
        self.f1 += 1;
    }
    pub fn m3(&self) -> u32 {
        self.f4 + self.f5
    }
    pub fn m4(&mut self) {
        self.f6 += 1;
        self.f4 += 1;
    }
    pub fn m5(&self) -> u32 {
        self.f1
    }
}
