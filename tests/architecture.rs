//! What spec 05 exists for: the gate is a test.
//!
//! `crate-graph.md` Option A prescribes a hand-written source scan in
//! `tests/boundaries.rs`. That file is a worse version of this tool,
//! maintained by hand, in every project that adopts the glossary. This is
//! what replaces it — and it is metatron pointed at the crate laid out
//! per `patterns-rust`, running in metatron's own suite.
//!
//! In a real project the whole file is four lines and `metatron` is a
//! **dev-dependency**, so it never appears in a release dependency graph.

use std::path::PathBuf;

fn conforming() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ports")
}

#[test]
fn architecture_holds() {
    metatron::check(conforming()).assert_no_new_violations();
}

#[test]
fn every_rule_can_actually_be_evaluated() {
    // The check that stops a green run from meaning nothing: a rule whose
    // premise is absent is not passing, and a suite that never asks will
    // not notice when a refactor deletes the last port.
    metatron::check(conforming()).assert_no_unevaluable();
}

#[test]
fn the_classifier_still_recognises_the_crate() {
    metatron::check(conforming()).assert_coverage_at_least(100.0);
}

#[test]
fn app_stays_decomposed() {
    // The cohesion ratchet, applied to one named type by its author.
    metatron::cohesion(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cohesion"))
        .assert_max_components("Split", 2);
}
