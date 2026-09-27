//! An impl block whose method set is generated. Analysing the half we can
//! see would produce a decomposition proposal for a type that does not
//! exist as written.

macro_rules! accessors {
    () => {
        pub fn generated(&self) -> u32 {
            self.p + self.q
        }
    };
}

pub struct Tainted {
    p: u32,
    q: u32,
    r: u32,
    s: u32,
    t: u32,
}

impl Tainted {
    accessors!();

    pub fn only_r(&self) -> u32 {
        self.r
    }
    pub fn only_s(&self) -> u32 {
        self.s
    }
    pub fn only_t(&self) -> u32 {
        self.t
    }
}
