//! metatron — compile a Rust crate into a measured architecture model.
//!
//! The library is the primary interface and the CLI is a wrapper over it,
//! not the other way around. That ordering is the reason this project is
//! Rust rather than a Node tool pointed at Rust source:
//! `crate-graph.md` Option A prescribes a hand-written source scan in
//! `tests/boundaries.rs`, which is a worse version of this tool
//! maintained by hand in every project. Here it is a test:
//!
//! ```no_run
//! #[test]
//! fn architecture_holds() {
//!     metatron::check(".").assert_no_new_violations();
//! }
//!
//! #[test]
//! fn app_stays_decomposed() {
//!     metatron::cohesion(".").assert_max_components("App", 3);
//! }
//! ```
//!
//! Everything on this path is required to be test-safe: it writes nothing
//! to disk, prints nothing, and touches no network. A test that mutates
//! the working tree is a bad test.

pub mod baseline;
pub mod classify;
pub mod cohesion;
pub mod model;
pub mod rules;
pub mod scan;
pub mod scorecard;
pub mod views;

pub use model::Model;
pub use scorecard::Scorecard;

use anyhow::Result;
use std::path::Path;

/// Parse the crate rooted at `dir` (the directory holding `Cargo.toml`).
pub fn scan(dir: impl AsRef<Path>) -> Result<Model> {
    scan::scan(dir.as_ref())
}

/// Everything spec 04 and spec 05 know about a crate, in one value.
///
/// Panics rather than returning `Result`, because the calling context is
/// a `#[test]` and a failure to scan should read as a test failure with a
/// message, not as an unwrapped error.
pub fn check(dir: impl AsRef<Path>) -> Scorecard {
    match scorecard::build(dir.as_ref()) {
        Ok(s) => s,
        Err(e) => panic!(
            "metatron could not analyse {}: {e:#}",
            dir.as_ref().display()
        ),
    }
}

/// Spec 02's cohesion report, for locking in a decomposition as it lands.
pub fn cohesion(dir: impl AsRef<Path>) -> CohesionCheck {
    let dir = dir.as_ref();
    match scan(dir) {
        Ok(m) => CohesionCheck {
            report: cohesion::analyse(&m),
        },
        Err(e) => panic!("metatron could not analyse {}: {e:#}", dir.display()),
    }
}

pub struct CohesionCheck {
    pub report: cohesion::CohesionReport,
}

impl CohesionCheck {
    /// Assert a named type stays at or below `max` components.
    ///
    /// Spec 02 declines to gate cohesion globally, for good reasons that
    /// still hold. A threshold the author chose for one named type, in
    /// their own test file, is a different thing from a threshold the
    /// tool imposes everywhere — and it is what lets a refactor lock in
    /// each extraction as it lands.
    #[track_caller]
    pub fn assert_max_components(&self, ty: &str, max: usize) {
        let Some(t) = self
            .report
            .types
            .iter()
            .find(|t| t.name == ty || t.symbol == ty)
        else {
            panic!(
                "metatron: no type named `{ty}` was analysed.\n\
                 A type with no methods is not analysed at all (spec 02); \
                 check the name, or that it still has an impl block."
            );
        };
        let n = t.components.len().max(1);
        assert!(
            n <= max,
            "metatron: `{ty}` has {n} cohesion component(s), expected at most {max}.\n\
             {}:{}  {} fields, {} methods, LCOM4 {}, modularity {:.3}\n{}",
            t.file,
            t.line,
            t.field_count,
            t.method_count,
            t.lcom4,
            t.modularity,
            t.components
                .iter()
                .map(|c| format!(
                    "  - {:<14} {} fields, {} methods",
                    c.name.clone().unwrap_or_else(|| "(unnamed)".into()),
                    c.fields.len(),
                    c.methods.len()
                ))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}
