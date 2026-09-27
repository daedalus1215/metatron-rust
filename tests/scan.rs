//! Acceptance tests for `specs/01-symbol-model.md`.

use metatron::model::*;
use std::path::PathBuf;

fn fixture(name: &str) -> Model {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    metatron::scan(&p).expect("scan failed")
}

fn sym<'a>(m: &'a Model, id: &str) -> &'a Symbol {
    m.symbol(id).unwrap_or_else(|| panic!("no symbol {id}"))
}

fn local_edges(m: &Model, kind: EdgeKind) -> Vec<(&str, &str)> {
    m.edges
        .iter()
        .filter(|e| e.kind == kind)
        .filter_map(|e| match &e.to {
            EdgeTarget::Local { id } => Some((e.from.as_str(), id.as_str())),
            _ => None,
        })
        .collect()
}

// ------------------------------------------------------------ parse robustness

#[test]
fn parses_generics_lifetimes_turbofish_and_raw_strings() {
    let m = fixture("gnarly");
    let parse_failures: Vec<_> = m
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::ParseFailure)
        .collect();
    assert!(parse_failures.is_empty(), "{parse_failures:?}");

    // `Vec<Box<dyn Fn(&'a str) -> Result<(), Error>>>` — the shape that kills
    // a brace-counting scanner.
    let reg = sym(&m, "Registry");
    let handlers = reg.fields.iter().find(|f| f.name == "handlers").unwrap();
    assert!(
        handlers.ty_paths.iter().any(|p| p == "Error"),
        "generic args not walked: {:?}",
        handlers.ty_paths
    );

    // A raw string containing `impl Fake for Nothing {}` must not become a symbol.
    assert!(m.symbol("Fake").is_none());
    assert!(m.symbol("Nothing").is_none());

    // `where T: Clone + Send + 'static` on the impl block, and a turbofish body.
    let parse = sym(&m, "Registry::parse");
    assert_eq!(parse.kind, SymbolKind::Method);
    assert!(parse.self_fields.contains(&"cache".to_string()));
    assert!(parse.self_calls.contains(&"helper".to_string()));
}

#[test]
fn macro_generated_items_are_diagnosed_not_dropped_silently() {
    let m = fixture("gnarly");
    let macros: Vec<_> = m
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::MacroItem)
        .collect();
    assert!(!macros.is_empty(), "macro_rules! produced no diagnostic");
    // The struct it generates is genuinely invisible — that is the declared
    // blind spot, and the diagnostic is what makes it honest.
    assert!(m.symbol("Invisible").is_none());
}

#[test]
fn no_diagnostic_says_nothing_or_stops_halfway() {
    // A diagnostic is the tool admitting it could not do something, and it is
    // read in a terminal and in the views. "`T` in " tells the reader that a
    // path could not be placed and then stops, which is the one thing a
    // diagnostic must not do: it costs a line of output and returns nothing.
    // Checked over every fixture, because the empty case is exactly the one a
    // single fixture would miss.
    for dir in [
        "ports",
        "leaky",
        "gnarly",
        "mixed",
        "cfgd",
        "dual",
        "ws-package",
        "escape",
        "panics",
    ] {
        for d in &fixture(dir).diagnostics {
            let detail = d.detail.trim();
            assert!(
                !detail.is_empty(),
                "{dir}: {:?} has an empty detail",
                d.kind
            );
            assert!(
                !detail.ends_with(" in") && !detail.ends_with(" of") && !detail.ends_with(" from"),
                "{dir}: {:?} stops halfway: {detail:?}",
                d.kind
            );
        }
    }
}

#[test]
fn derives_are_captured() {
    let m = fixture("gnarly");
    let mode = sym(&m, "Mode");
    for d in ["Serialize", "Clone", "Copy", "PartialEq"] {
        assert!(mode.derives.iter().any(|x| x == d), "missing derive {d}");
    }
}

// ------------------------------------------------------------- entry points

/// Spec 07. Cargo builds a `[[bin]]` whose root is outside `src/`. A scanner
/// that only looks under `src/` misses it, and reports what it did scan as
/// though it were the crate.
#[test]
fn a_workspace_root_says_which_members_it_did_not_cross_into() {
    // A crate that is also a workspace root has its own source, so it is
    // scanned — and the member beside it is a separate crate that is not.
    let m = fixture("ws-package");
    assert!(
        m.symbol("root_fn").is_some(),
        "the root package should still be scanned"
    );
    assert!(
        m.symbol("inner_fn").is_none(),
        "the scan crossed into a workspace member"
    );

    let named: Vec<_> = m
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::TargetSkipped)
        .filter(|d| d.detail.contains("workspace members"))
        .collect();
    assert_eq!(named.len(), 1, "{named:?}");
    assert!(named[0].detail.contains("inner"), "{}", named[0].detail);
}

#[test]
fn a_virtual_manifest_is_an_error_naming_the_members() {
    // Nothing to model, and a model with zero symbols named "unknown" is a
    // worse answer than a sentence saying which directory to point at.
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ws-virtual");
    let err = metatron::scan(&p).expect_err("a virtual manifest should not scan");
    let msg = format!("{err:#}");
    assert!(msg.contains("workspace root"), "{msg}");
    assert!(msg.contains("core") && msg.contains("cli"), "{msg}");
}

#[test]
fn a_declared_bin_whose_root_is_outside_src_is_scanned() {
    let m = fixture("declared");

    let cli = sym(&m, "(bin:cli)::main");
    assert_eq!(cli.kind, SymbolKind::Fn);
    assert_eq!(
        cli.file, "../tools/cli.rs",
        "a target outside src/ must still be relative to the model root"
    );

    // And the library is still the crate root, so `use declared::helper` in
    // the binary resolves rather than becoming a diagnostic.
    let unresolved: Vec<_> = m
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::UnresolvedPath)
        .collect();
    assert!(unresolved.is_empty(), "{unresolved:?}");
}

#[test]
fn a_target_that_could_not_be_scanned_is_named() {
    let m = fixture("declared");

    let skipped: Vec<_> = m
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::TargetSkipped)
        .collect();

    // The declared path that does not exist.
    assert!(
        skipped
            .iter()
            .any(|d| d.detail.contains("`gone`") && d.detail.contains("tools/gone.rs")),
        "a declared path that does not exist was not reported: {skipped:?}"
    );

    // The example, which exists and is deliberately not architecture. A
    // decision not to look is still a decision, and it gets a diagnostic.
    assert!(
        skipped.iter().any(|d| d.detail.contains("example `demo`")),
        "not scanning an example was not reported: {skipped:?}"
    );
}

/// Spec 07. The scanner used to take the first of `main.rs` / `lib.rs` it
/// found, so a crate with both targets was analysed minus its entire library —
/// and reported the result as though it were the whole crate.
#[test]
fn a_dual_target_crate_scans_both_roots() {
    let m = fixture("dual");

    assert_eq!(
        m.stats.files,
        2,
        "expected both src/lib.rs and src/main.rs, got {:?}",
        m.modules.iter().map(|x| &x.file).collect::<Vec<_>>()
    );

    // A symbol from each target. The library's port is the one that matters:
    // if `lib.rs` was skipped, `Greeter` is absent and every rule that needs a
    // port silently has no premise.
    assert_eq!(sym(&m, "Greeter").kind, SymbolKind::Trait);
    assert_eq!(sym(&m, "Console").kind, SymbolKind::Struct);
    assert_eq!(sym(&m, "greet_all").kind, SymbolKind::Fn);

    // And one from the binary, so the test cannot pass by scanning the crate
    // root alone and calling it a library. The binary is not the crate root —
    // the library is — so its symbols carry a synthetic prefix that no path in
    // source can collide with.
    assert_eq!(sym(&m, "(bin:main)::main").kind, SymbolKind::Fn);
}

#[test]
fn a_dual_target_scan_says_so() {
    let m = fixture("dual");

    let names: Vec<&str> = m.stats.targets.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["dual", "main"], "the summary must name both roots");
    assert_eq!(m.stats.targets[0].kind, "lib");
    assert_eq!(m.stats.targets[0].file, "lib.rs");
    assert_eq!(m.stats.targets[1].kind, "bin");
    assert_eq!(m.stats.targets[1].file, "main.rs");

    // Two roots, two module-tree nodes, and neither id borrowed from the other.
    let roots: Vec<&str> = m
        .modules
        .iter()
        .filter(|x| x.parent.is_none())
        .map(|x| x.id.as_str())
        .collect();
    assert_eq!(roots, ["(crate)", "(bin:main)"]);
}

#[test]
fn the_binary_resolves_the_library_by_its_crate_name() {
    let m = fixture("dual");

    // `use dual::{greet_all, Console, Greeter}` in the binary points at the
    // library's own crate name. A resolver that does not know the crate calls
    // itself an external crate and the path becomes a diagnostic.
    let unresolved: Vec<_> = m
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::UnresolvedPath)
        .collect();
    assert!(
        unresolved.is_empty(),
        "the binary could not reach its own library: {unresolved:?}"
    );

    // The `Impl` binding recorded in the library must still be bound, which it
    // only is if the library's symbols were in the index.
    let binding = m
        .impls
        .iter()
        .find(|b| b.trait_path.ends_with("Greeter"))
        .expect("the Impl binding in lib.rs was not recorded");
    assert_eq!(binding.trait_id.as_deref(), Some("Greeter"));
    assert_eq!(binding.type_id.as_deref(), Some("Console"));
}

// ------------------------------------------------------------- the port seam

#[test]
fn impl_trait_for_type_is_an_edge_pointing_at_the_port() {
    let m = fixture("ports");

    let bindings: Vec<_> = m
        .impls
        .iter()
        .filter(|b| b.trait_path.ends_with("ActivityStore"))
        .collect();
    assert_eq!(bindings.len(), 2, "expected sqlite + mem impls");

    for b in &bindings {
        assert_eq!(
            b.trait_id.as_deref(),
            Some("domain::ports::activity_store::ActivityStore"),
            "port trait did not resolve to the domain trait"
        );
    }

    // The edge runs concrete -> trait: infrastructure back up to domain.
    let impls = local_edges(&m, EdgeKind::Impl);
    assert!(impls.contains(&(
        "infra::sqlite::SqliteActivityStore",
        "domain::ports::activity_store::ActivityStore"
    )));
    assert!(impls.contains(&(
        "infra::mem::MemStore",
        "domain::ports::activity_store::ActivityStore"
    )));
}

#[test]
fn impl_bound_distinguishes_port_from_concrete() {
    let m = fixture("ports");

    // `&impl ActivityStore` is a Bound, not a Sig — this is what separates
    // "calls through the port" from naming a concrete store.
    let bounds = local_edges(&m, EdgeKind::Bound);
    assert!(bounds.contains(&(
        "domain::use_cases::activity::start_activity",
        "domain::ports::activity_store::ActivityStore"
    )));
    assert!(bounds.contains(&(
        "domain::use_cases::activity::start_activity",
        "domain::ports::clock::Clock"
    )));

    // The use case must not reach a concrete store by any edge kind.
    let uc = "domain::use_cases::activity::start_activity";
    let reaches_concrete = m.edges.iter().any(|e| {
        e.from == uc && matches!(&e.to, EdgeTarget::Local { id } if id.contains("Sqlite"))
    });
    assert!(!reaches_concrete, "use case named a concrete store");

    // The composition root, and only it, names the concrete.
    let names_sqlite: Vec<&str> = m
        .edges
        .iter()
        .filter(|e| matches!(&e.to, EdgeTarget::Local { id } if id.contains("SqliteActivityStore")))
        .map(|e| e.from.as_str())
        .collect();
    assert!(names_sqlite
        .iter()
        .all(|f| f.starts_with("main") || f.starts_with("infra") || *f == "(crate)"));
}

#[test]
fn infra_reaches_its_io_crate_and_domain_does_not() {
    let m = fixture("ports");
    let extern_from = |sym_prefix: &str, krate: &str| {
        m.edges.iter().any(|e| {
            e.from.starts_with(sym_prefix)
                && matches!(&e.to, EdgeTarget::Extern { krate: k, .. } if k == krate)
        })
    };
    assert!(extern_from("infra::sqlite", "rusqlite"));
    assert!(!extern_from("domain::", "rusqlite"));
}

#[test]
fn module_tree_follows_mod_declarations_into_directories() {
    let m = fixture("ports");
    let ids: Vec<&str> = m.modules.iter().map(|x| x.id.as_str()).collect();
    for want in [
        "domain",
        "domain::ports",
        "domain::ports::activity_store",
        "domain::use_cases::activity",
        "infra::sqlite",
        "application::cli",
    ] {
        assert!(ids.contains(&want), "missing module {want} in {ids:?}");
    }
}

// ------------------------------------------------------------------- arioch
//
// The real target. These assert the state described in the specs README: a
// pre-refactor crate with no ports at all. They are skipped rather than failed
// when the checkout is absent, so the suite runs anywhere.

fn arioch() -> Option<Model> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../arioch")
        .canonicalize()
        .ok()?;
    if !p.join("Cargo.toml").exists() {
        return None;
    }
    metatron::scan(&p).ok()
}

#[test]
fn arioch_app_is_one_type_with_41_fields_and_48_methods() {
    let Some(m) = arioch() else { return };
    let app = sym(&m, "app::App");
    assert_eq!(app.fields.len(), 41);
    assert_eq!(
        m.symbols
            .iter()
            .filter(|s| s.parent.as_deref() == Some("app::App"))
            .count(),
        48
    );
    assert_eq!(m.modules.len(), 8);
}

#[test]
fn arioch_has_no_locally_defined_port() {
    let Some(m) = arioch() else { return };
    // Two `impl Default for _` exist; neither trait is defined in this crate.
    assert!(
        m.impls.iter().all(|b| b.trait_id.is_none()),
        "a locally-defined trait is implemented — arioch grew a port"
    );
    assert_eq!(
        m.symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Trait)
            .count(),
        0
    );
}

#[test]
fn arioch_io_leak_into_app_is_visible() {
    // `patterns-rust/infrastructure/adapter.md` calls this "the defining
    // violation for arioch". The model has to be able to see it.
    let Some(m) = arioch() else { return };
    let leaks: Vec<&str> = m
        .edges
        .iter()
        .filter(|e| e.file == "app.rs")
        .filter_map(|e| match &e.to {
            EdgeTarget::Extern { path, .. }
                if path.starts_with("std::fs") || path.starts_with("std::process") =>
            {
                Some(path.as_str())
            }
            _ => None,
        })
        .collect();
    assert!(leaks.contains(&"std::fs::read_to_string"));
    assert!(leaks.contains(&"std::process::Command::new"));
}

#[test]
fn arioch_global_mutable_state_is_captured() {
    // `design-philosophy.md` principle 3 names CONFIG_OVERRIDE as the
    // anti-pattern to refuse. `no-global-mut` (spec 04) needs to see it.
    let Some(m) = arioch() else { return };
    let s = sym(&m, "config::CONFIG_OVERRIDE");
    assert_eq!(s.kind, SymbolKind::Static);
    assert!(s.fields[0].ty.contains("Mutex"));
}

#[test]
fn arioch_scans_without_diagnostics() {
    let Some(m) = arioch() else { return };
    assert!(
        m.diagnostics.is_empty(),
        "unexpected diagnostics: {:?}",
        &m.diagnostics[..m.diagnostics.len().min(5)]
    );
}

#[test]
fn lcom_inputs_are_populated() {
    // Spec 02 consumes these. Collected during the spec 01 visit.
    let Some(m) = arioch() else { return };
    let methods: Vec<&Symbol> = m
        .symbols
        .iter()
        .filter(|s| s.parent.as_deref() == Some("app::App"))
        .collect();
    let with_fields = methods.iter().filter(|s| !s.self_fields.is_empty()).count();
    let with_calls = methods.iter().filter(|s| !s.self_calls.is_empty()).count();
    assert!(with_fields > 30, "only {with_fields} methods touch a field");
    assert!(with_calls > 20, "only {with_calls} methods call a sibling");
}

// --------------------------------------------------------------- cfg honesty

#[test]
fn a_cfg_gated_item_is_recorded_and_the_model_says_it_may_not_compile() {
    let m = fixture("cfgd");

    // Recorded, not evaluated: the symbol is in the model either way, carrying
    // its predicate. Removing gated code from the picture would hide more than
    // it reveals.
    let fast = sym(&m, "Fast");
    assert_eq!(fast.kind, SymbolKind::Struct);
    assert!(
        fast.cfg.iter().any(|c| c.contains("fast")),
        "the predicate was dropped: {:?}",
        fast.cfg
    );

    // And the model admits the rest of it. One diagnostic per *file*, naming
    // the count: per-symbol would be six lines of noise that trains the reader
    // to skip the section.
    let cfgd: Vec<&Diagnostic> = m
        .diagnostics
        .iter()
        .filter(|d| d.kind == DiagnosticKind::CfgExcluded)
        .collect();
    assert_eq!(
        cfgd.len(),
        2,
        "expected one per file, not one per symbol: {cfgd:?}"
    );

    let lib = cfgd.iter().find(|d| d.file == "lib.rs").expect("lib.rs");
    assert!(lib.line > 0, "a file diagnostic should point at a line");
    assert!(
        lib.detail.contains("2 item(s)"),
        "the count should match the gated symbols: {}",
        lib.detail
    );

    let platform = cfgd
        .iter()
        .find(|d| d.file == "platform.rs")
        .expect("platform.rs");
    assert!(
        platform.detail.contains("3 item(s)"),
        "a second file is counted separately: {}",
        platform.detail
    );

    for d in &cfgd {
        assert!(!d.detail.is_empty(), "{d:?}");
        assert!(
            d.detail.contains("may not compile") || d.detail.contains("does not compile"),
            "the diagnostic should say what it means: {}",
            d.detail
        );
    }
}

#[test]
fn a_file_with_no_cfg_produces_no_such_diagnostic() {
    // The negative control. Without it, a scanner that emitted one CfgExcluded
    // per file unconditionally would pass the test above.
    let m = fixture("dual");
    assert!(
        m.diagnostics
            .iter()
            .all(|d| d.kind != DiagnosticKind::CfgExcluded),
        "a crate with no cfg attributes was reported: {:?}",
        m.diagnostics
    );
}

// ------------------------------------------------- a suite the scanner can see

#[test]
fn a_crates_tests_are_part_of_the_crate() {
    let m = fixture("suite");

    // Two discovered: `acceptance.rs` and `nested/main.rs`.
    let tests: Vec<_> = m
        .stats
        .targets
        .iter()
        .filter(|t| t.kind == "test")
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(tests, ["acceptance", "nested"], "test targets: {tests:?}");

    // Every symbol in a test target is test code, annotated or not: the target
    // is the premise, not the `#[test]` attribute. The paths carry the `../`
    // the model uses for anything outside `src/`, so a reader can see that
    // these files are not part of the scanned root.
    let in_tests: Vec<_> = m
        .symbols
        .iter()
        .filter(|s| s.file.starts_with("../tests/"))
        .collect();
    assert!(!in_tests.is_empty(), "the suite produced no symbols at all");
    for s in &in_tests {
        assert!(s.is_test, "{} is in tests/ but not marked", s.id);
    }
    assert!(in_tests.iter().any(|s| s.id.contains("the_log_records")));

    // And the source under `src/` is not test code. A crate with a test target
    // is not itself a test.
    let log = m
        .symbols
        .iter()
        .find(|s| s.id.ends_with("::log"))
        .expect("log");
    assert!(!log.is_test, "{} is src code marked as test", log.id);
}

#[test]
fn a_declared_test_target_is_scanned_and_a_missing_one_is_named() {
    let m = fixture("declared-test");

    let names: Vec<_> = m
        .stats
        .targets
        .iter()
        .filter(|t| t.kind == "test")
        .map(|t| t.name.as_str())
        .collect();
    assert!(
        names.contains(&"outside"),
        "declared test not scanned: {names:?}"
    );

    // The declared path pointed outside `tests/`, and was scanned anyway: a
    // target cargo would build is a target this scanner opens.
    let outside = m
        .stats
        .targets
        .iter()
        .find(|t| t.name == "outside")
        .expect("the declared target");
    assert_eq!(outside.file, "extra.rs", "a declared path under src/");
    let sym = m
        .symbols
        .iter()
        .find(|s| s.id.contains("declared_helper"))
        .expect("symbol from the declared test target");
    assert!(sym.is_test, "{} is a test target but not marked", sym.id);

    // A declared test with no file is named, not dropped.
    let skipped = m
        .diagnostics
        .iter()
        .find(|d| d.detail.contains("test `ghost`"))
        .expect("a declared test whose path does not exist must be named");
    assert_eq!(skipped.kind, DiagnosticKind::TargetSkipped);
    assert!(
        skipped.detail.contains("does not exist"),
        "the diagnostic must say why: {}",
        skipped.detail
    );
}
