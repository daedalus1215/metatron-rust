use crate::ports::{Clock, Store};

pub struct Wall;

impl Clock for Wall {
    fn now(&self) -> u64 {
        0
    }
}

pub struct Sqlite {
    conn: rusqlite::Connection,
}

impl Sqlite {
    pub fn open() -> Self {
        unimplemented!()
    }

    pub fn vacuum(&self) {}
}

impl Store for Sqlite {
    fn save(&self, _at: u64) {}
    fn load(&self) -> u64 {
        0
    }
}

impl std::fmt::Display for Sqlite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "sqlite")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake: not a binding of `Clock`, so `Wall` stays the only one.
    struct Frozen;

    impl Clock for Frozen {
        fn now(&self) -> u64 {
            7
        }
    }
}
