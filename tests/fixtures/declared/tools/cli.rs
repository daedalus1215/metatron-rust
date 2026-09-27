//! A binary root outside `src/`. Reaches the library by crate name, so the
//! cross-target path is exercised as well as the unusual file location.

use declared::helper;

fn main() {
    println!("{}", helper());
}
