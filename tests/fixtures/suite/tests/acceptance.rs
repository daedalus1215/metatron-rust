use suite::application::{default_store, start};
use suite::domain::{helper, ActivityStore};

#[test]
fn the_log_records() {
    start(&default_store());
    assert_eq!(helper(), 7);
}

#[test]
fn the_store_is_usable() {
    default_store().record("x");
}

// Not a `#[test]` and not annotated: the only thing marking this as test code
// is the target it lives in. A scanner that forgot test targets would classify
// it like any other symbol, which is the regression this fixture exists for.
struct Fixture {
    name: &'static str,
}

fn make_fixture() -> Fixture {
    Fixture { name: "acceptance" }
}

fn log_via_fixture() {
    start(&default_store());
    let _ = make_fixture().name;
}

// Named like a domain function on purpose: if the classifier ever stopped
// treating a test target as test code, this would classify as `domain` and the
// guard below would catch it. The name is the trap.
fn helper() -> u32 {
    9
}
