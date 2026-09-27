// Everything here is a shape that a regex-based scanner mis-reads.
use serde::Serialize;
use std::collections::HashMap;

pub struct Registry<'a, T: Clone + Send> {
    pub handlers: Vec<Box<dyn Fn(&'a str) -> Result<(), Error>>>,
    pub cache: HashMap<String, Vec<Option<T>>>,
    pub raw: &'a str,
}

pub struct Error;

impl<'a, T> Registry<'a, T>
where
    T: Clone + Send + 'static,
{
    pub fn new(raw: &'a str) -> Self {
        Self { handlers: Vec::new(), cache: HashMap::new(), raw }
    }

    pub fn parse(&self) -> Result<Vec<T>, Error> {
        let _s = r#"a "quoted" ::path:: that is not code"#;
        let _t = r"impl Fake for Nothing {}";
        let ids = self.cache.keys().collect::<Vec<_>>();
        let _n = ids.len();
        self.helper();
        Ok(Vec::new())
    }

    fn helper(&self) -> Option<&'a str> {
        Some(self.raw)
    }
}

pub trait Store {
    fn get(&self, k: &str) -> Option<String>;
}

macro_rules! make_thing {
    ($n:ident) => { pub struct $n; };
}
make_thing!(Invisible);

#[derive(Serialize, Clone, Copy, PartialEq)]
pub enum Mode { A, B }

fn main() {}
