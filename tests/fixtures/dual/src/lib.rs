//! The library target. A port, its adapter, and a use-case that calls through
//! the port rather than the concrete.

/// The port. Calling through this is the whole point.
pub trait Greeter {
    fn greeting(&self, name: &str) -> String;
}

/// An adapter. `Console` knows nothing about the caller.
pub struct Console {
    pub prefix: String,
}

impl Greeter for Console {
    fn greeting(&self, name: &str) -> String {
        format!("{}{name}", self.prefix)
    }
}

/// A use-case. Takes the port, not the concrete.
pub fn greet_all(g: &dyn Greeter, names: &[&str]) -> Vec<String> {
    names.iter().map(|n| g.greeting(n)).collect()
}
