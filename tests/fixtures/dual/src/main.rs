//! The binary target. Reaches into the library by its crate name, which is
//! the path a resolver has to place or admit it cannot.

use dual::{greet_all, Console, Greeter};

fn main() {
    let console = Console { prefix: "hello, " };
    for line in greet_all(&console, &["world"]) {
        println!("{line}");
    }
}
