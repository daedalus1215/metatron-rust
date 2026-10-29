pub trait Clock {
    fn now(&self) -> u64;
}

pub trait Store {
    fn save(&self, at: u64);
    fn load(&self) -> u64;
}
