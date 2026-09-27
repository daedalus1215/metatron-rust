pub trait Clock {
    fn now_ms(&self) -> i64;
}
