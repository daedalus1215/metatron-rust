//! Acceptance tests for `specs/03-classifier-and-config.md`.

use metatron::classify::{classify, Classified, Config};
use metatron::model::Model;
use std::path::PathBuf;

fn at(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn run(dir: PathBuf) -> (Model, Config, Classified) {
    let m = metatron::scan(&dir).expect("scan failed");
    let c = Config::load(&dir).expect("config failed");
    let r = classify(&m, &c);
    (m, c, r)
}

fn fixture(name: &str) -> (Model, Config, Classified) {
    run(at(name))
}

/// The real checkouts live beside this one.
fn sibling(name: &str) -> Option<(Model, Config, Classified)> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join(name);
    p.join("Cargo.toml").exists().then(|| run(p))
}

// ------------------------------------------------- the conforming crate

#[test]
fn a_crate_laid_out_per_the_glossary_classifies_completely() {
    let (_, _, r) = fixture("ports");
    assert_eq!(
        r.coverage.classified, r.coverage.total,
        "unmatched: {:?}",
        r.unmatched.iter().map(|u| &u.symbol).collect::<Vec<_>>()
    );
    for (id, want) in [
        ("domain::ports::activity_store::ActivityStore", "port"),
        ("domain::ports::clock::Clock", "port"),
        ("domain::use_cases::activity::start_activity", "use-case"),
        ("domain::Activity", "entity"),
        ("domain::StartOutcome", "value-object"),
        ("infra::sqlite::SqliteActivityStore", "store"),
        ("infra::fs::RealFs", "adapter"),
        ("application::cli::cmd_start", "command-handler"),
        ("application::cli::ActivityRow", "view-model"),
        ("main", "composition-root"),
    ] {
        assert_eq!(r.pattern_of(id), Some(want), "{id}");
    }
}

#[test]
fn the_two_infra_patterns_are_told_apart_by_what_they_talk_to() {
    // store.md and adapter.md describe the same shape. `SqliteActivityStore`
    // holds a `rusqlite::Connection`; `RealFs` calls `std::fs`. Nothing but
    // the extern reach separates them.
    let (_, _, r) = fixture("ports");
    assert_eq!(r.pattern_of("infra::sqlite::SqliteActivityStore"), Some("store"));
    assert_eq!(r.pattern_of("infra::fs::RealFs"), Some("adapter"));
    assert_eq!(r.layer_of("infra::sqlite::SqliteActivityStore"), Some("infrastructure"));
    assert_eq!(r.layer_of("infra::fs::RealFs"), Some("infrastructure"));
}

#[test]
fn an_infra_impl_that_names_no_external_crate_is_not_guessed() {
    // `MemStore` is a fake: it implements a port and talks to nothing. It
    // matches the shape of both store and adapter and the externs of
    // neither. The label is the group; the ambiguity is recorded.
    let (_, _, r) = fixture("ports");
    for id in ["infra::mem::MemStore", "infra::mem::FixedClock"] {
        assert_eq!(r.pattern_of(id), Some("infra-impl"), "{id}");
        assert!(r.by_symbol[id].ambiguous, "{id} should be flagged ambiguous");
        assert!(r.ambiguities.contains(&id.to_string()));
    }
}

// --------------------------------------------- per symbol, not per file

#[test]
fn two_symbols_in_one_file_keep_their_own_layers() {
    // The whole reason classification is per symbol. A file-level verdict
    // would report `app` as one layer and hide the other two.
    let (_, _, r) = fixture("mixed");
    assert_eq!(r.layer_of("app::Mode"), Some("domain"));
    assert_eq!(r.layer_of("app::handle_key"), Some("application"));
    assert_eq!(r.layer_of("app::load_config"), Some("infrastructure"));
}

#[test]
fn a_module_spanning_layers_is_reported_and_a_single_layer_one_is_not() {
    // spec 02, detector 2 — dark until this spec existed.
    let (m, _, r) = fixture("mixed");
    let mixed = r.mixed_layer_modules(&m);
    let app = mixed.iter().find(|(id, _)| id == "app").expect("app not reported");
    assert_eq!(app.1.len(), 3, "{:?}", app.1);
    assert!(
        !mixed.iter().any(|(id, _)| id == "pure"),
        "the control module was reported: {mixed:?}"
    );
}

// ------------------------------------------------------------- the config

#[test]
fn add_pattern_prepends_and_the_profile_still_applies() {
    // The `mixed` fixture adds four shape-only rules and inherits the rest.
    // `main` is still composition-root, which only the profile declares.
    let (_, cfg, r) = fixture("mixed");
    assert_eq!(cfg.extends.as_deref(), Some("patterns-rust"));
    assert_eq!(cfg.patterns[0].id, "key-handler", "add_pattern did not prepend");
    assert!(cfg.patterns.iter().any(|p| p.id == "port"), "profile was dropped");
    assert_eq!(r.pattern_of("main"), Some("composition-root"));
    // Inherited from the profile even though the project never named them.
    assert!(!cfg.flow.is_empty());
    assert!(cfg.externs.contains_key("io"));
}

#[test]
fn a_crate_with_no_config_gets_the_glossary_unmodified() {
    let cfg = Config::load(&at("ports")).unwrap();
    let profile = Config::profile();
    assert_eq!(cfg.patterns.len(), profile.patterns.len());
    assert_eq!(cfg.flow, profile.flow);
}

#[test]
fn dropping_the_port_pattern_drops_the_tools_confidence() {
    // Confidence tracks the config, not the code. Same crate, same ports,
    // no rule to recognise them.
    let dir = at("ports");
    let m = metatron::scan(&dir).unwrap();

    let full = classify(&m, &Config::load(&dir).unwrap());
    assert_eq!(full.coverage.ports, 3);

    let mut cfg = Config::load(&dir).unwrap();
    cfg.patterns.retain(|p| p.id != "port");
    let without = classify(&m, &cfg);

    assert_eq!(without.coverage.ports, 0);
    assert!(
        without.coverage.classified < full.coverage.classified,
        "coverage did not fall: {} vs {}",
        without.coverage.classified,
        full.coverage.classified
    );
    // `implements_port` can no longer be satisfied, so store and adapter
    // stop matching too — the premise is gone, not merely unproven.
    assert_ne!(without.pattern_of("infra::sqlite::SqliteActivityStore"), Some("store"));
}

// -------------------------------------------------------------- arioch

#[test]
fn arioch_classifies_at_approximately_zero_and_says_so() {
    // Zero is the correct answer and must not be suppressed, rounded away,
    // or reported as a pass. arioch has no `domain/`, no `infra/`, no
    // `application/`, and no trait anywhere.
    let Some((_, _, r)) = sibling("arioch") else { return };
    assert_eq!(r.coverage.ports, 0, "arioch has no locally defined trait");
    assert!(
        r.coverage.ratio() < 0.05,
        "expected ~0% coverage, got {:.1}%",
        r.coverage.ratio() * 100.0
    );
    // Only `fn main` matches, and only because it is the composition root
    // by position. Everything else is unmatched, with a file and line.
    for name in ["App", "Registry", "Config"] {
        let u = r
            .unmatched
            .iter()
            .find(|u| u.name == name)
            .unwrap_or_else(|| panic!("{name} should be reported unmatched"));
        assert!(!u.file.is_empty() && u.line > 0);
    }
}

#[test]
fn the_composition_root_rule_does_not_swallow_the_file_it_names() {
    // A path-only rule on `main.rs` classifies everything that happens to
    // sit there. In arioch that is 17 symbols, and it turns a 1% coverage
    // figure into 20% — the tool flattering the code again.
    let Some((_, _, r)) = sibling("arioch") else { return };
    let roots = r
        .by_symbol
        .values()
        .filter(|c| c.pattern == "composition-root" && !c.inherited)
        .count();
    assert_eq!(roots, 1, "composition-root matched more than `fn main`");
}

#[test]
fn methods_inherit_their_type_and_do_not_inflate_coverage() {
    let (_, _, r) = fixture("ports");
    let m = &r.by_symbol["infra::sqlite::SqliteActivityStore::open"];
    assert_eq!(m.pattern, "store");
    assert!(m.inherited);
    // 48 methods on one god object must not count as 48 classified units.
    assert!(
        !r.coverage.by_pattern.values().any(|n| *n > r.coverage.total),
        "inherited classifications leaked into the totals"
    );
}
