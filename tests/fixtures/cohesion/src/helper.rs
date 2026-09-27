//! A stateless private helper called from two places. It must join the
//! component of its callers, not become one of its own.

pub struct Helper {
    x1: u32,
    x2: u32,
    x3: u32,
    x4: u32,
    x5: u32,
}

impl Helper {
    pub fn one(&self) -> u32 {
        self.x1 + self.x2 + self.norm()
    }
    pub fn two(&self) -> u32 {
        self.x3 + self.norm()
    }
    pub fn three(&self) -> u32 {
        self.x4 + self.x5 + self.x1
    }
    fn norm(&self) -> u32 {
        7
    }
}
