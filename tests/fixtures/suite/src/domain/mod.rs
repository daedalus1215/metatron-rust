pub mod activity;

pub trait ActivityStore {
    fn record(&self, name: &str);
}

pub fn helper() -> u32 {
    7
}
