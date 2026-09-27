//! Two jobs sharing a name and nothing else. LCOM4 = 2 with no threshold
//! involved: the components are genuinely disjoint.

pub struct Split {
    a1: u32,
    a2: u32,
    a3: u32,
    b1: u32,
    b2: u32,
    b3: u32,
}

impl Split {
    pub fn read_a(&self) -> u32 {
        self.a1 + self.a2
    }
    pub fn bump_a(&mut self) {
        self.a3 += 1;
        self.a1 += 1;
    }
    pub fn read_b(&self) -> u32 {
        self.b1 + self.b2
    }
    pub fn bump_b(&mut self) {
        self.b3 += 1;
        self.b1 += 1;
    }
    pub fn peek_a(&self) -> u32 {
        self.a1
    }
}
