//! Acceptance tests for `specs/04-conformance-rules.md`.

use metatron::classify::{classify, Config};
use metatron::rules::{check, Finding, Kind, Report, Status, Tier};
use std::path::PathBuf;

fn run(dir: PathBuf) -> Report {
    let m = metatron::scan(&dir).expect("scan failed");
    let cfg = Config::load(&dir).expect("config failed");
    let c = classify(&m, &cfg);
    check(&m, &cfg, &c)
}

fn fixture(name: &str) -> Report {
    run(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name))
}

fn sibling(name: &str) -> Option<Report> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join(name);
    p.join("Cargo.toml").exists().then(|| run(p))
}

fn rule<'a>(r: &'a Report, id: &str) -> &'a Finding {
    r.findings
        .iter()
        .find(|f| f.id == id)
        .unwrap_or_else(|| panic!("no rule {id}"))
}

// -------------------------------------------------- the conforming crate

#[test]
fn the_conforming_crate_violates_no_gating_rule() {
    let r = fixture("ports");
    let failed: Vec<&str> = r.findings.iter().filter(|f| f.failed()).map(|f| f.id).collect();
    assert!(failed.is_empty(), "gating failures on a conforming crate: {failed:?}");
}

#[test]
fn every_decidable_rule_evaluates_against_the_conforming_crate() {
    // A fixture that leaves rules unevaluable is not exercising them.
    let r = fixture("ports");
    let dark: Vec<&str> = r
        .findings
        .iter()
        .filter(|f| f.kind == Kind::Decidable && f.status == Status::Unevaluable)
        .map(|f| f.id)
        .collect();
    assert!(dark.is_empty(), "unevaluable against a conforming crate: {dark:?}");
}

#[test]
fn the_inversion_arrow_is_counted_as_the_architecture_working() {
    // `impl <domain trait> for <infra struct>` runs backwards against the
    // flow. That is the point of it, not a violation to tolerate.
    let r = fixture("ports");
    let f = rule(&r, "dependency-inversion");
    assert_eq!(f.status, Status::Upheld);
    assert_eq!(f.instances.len(), 5, "{:?}", f.instances);
}

// ------------------------------------------------------- the leaky crate

#[test]
fn every_decidable_rule_fires_on_the_crate_built_to_break_it() {
    let r = fixture("leaky");
    for id in [
        "domain-no-io",
        "concrete-outside-root",
        "call-through-port",
        "flow-skip",
        "no-same-level",
        "port-signature-purity",
        "no-global-mut",
        "dto-in-domain",
        "use-case-verb",
        "time-injected",
        "converter-is-pure",
        "port-has-fake",
    ] {
        let f = rule(&r, id);
        assert_eq!(f.status, Status::Violated, "{id} did not fire");
        assert!(!f.instances.is_empty());
        for i in &f.instances {
            assert!(!i.file.is_empty() && i.line > 0, "{id} has no location");
        }
    }
}

#[test]
fn every_heuristic_fires_and_none_of_them_gates() {
    // The split that keeps the tool alive: a ratchet that fails CI on a
    // guess is switched off within a week, and takes the decidable rules
    // with it.
    let r = fixture("leaky");
    for id in ["store-decides", "handler-decides", "service-wraps-one", "fat-trait", "renders-off-store"] {
        let f = rule(&r, id);
        assert_eq!(f.kind, Kind::Heuristic, "{id}");
        assert!(!f.gate, "{id} gates");
        assert!(!f.failed(), "{id} counted as a failure");
    }
    let fired: Vec<&str> = r
        .findings
        .iter()
        .filter(|f| f.kind == Kind::Heuristic && f.status == Status::Violated)
        .map(|f| f.id)
        .collect();
    assert!(fired.len() >= 3, "expected the heuristics to fire: {fired:?}");
    // And the exit code ignores them entirely.
    let heuristic_failures = r.findings.iter().filter(|f| f.kind == Kind::Heuristic && f.failed()).count();
    assert_eq!(heuristic_failures, 0);
}

#[test]
fn the_violation_the_glossary_cares_most_about_is_reported_and_never_gates() {
    // `Db::start` returning `StartOutcome::AlreadyRunning` — cited on
    // store.md, use-case.md and service.md. It is semantic, so the best
    // available proxy is fallible, so it cannot gate.
    let r = fixture("leaky");
    let f = rule(&r, "store-decides");
    assert_eq!(f.status, Status::Violated);
    assert!(f.instances.iter().any(|i| i.detail.contains("StartOutcome")));
    assert!(!f.gate);
}

#[test]
fn naming_a_concrete_fires_both_rules_that_cover_it() {
    // `fn start_activity(store: &SqliteActivityStore)` is two violations:
    // the parameter is not a bound, and the concrete is named outside
    // infra/ and the root.
    let r = fixture("leaky");
    let ctp = rule(&r, "call-through-port");
    let cor = rule(&r, "concrete-outside-root");
    assert!(ctp
        .instances
        .iter()
        .any(|i| i.from == "domain::use_cases::activity::start_activity"));
    assert!(cor.instances.iter().any(|i| i.to.ends_with("SqliteActivityStore")));
}

#[test]
fn a_missing_fake_warns_and_does_not_fail() {
    let r = fixture("leaky");
    let f = rule(&r, "port-has-fake");
    assert_eq!(f.status, Status::Violated);
    assert!(!f.gate, "port-has-fake must warn, not fail");
    assert!(!f.failed());
}

// -------------------------------------------------------- the exemptions

#[test]
fn impl_edges_are_exempt_from_the_flow_and_layer_rules() {
    // `impl ActivityStore for SqliteActivityStore` runs from
    // infrastructure to domain. Against the flow that reads as a maximal
    // violation; in hexagonal architecture it is the whole point.
    let r = fixture("ports");
    for id in ["flow-skip", "domain-no-application", "infra-no-application"] {
        let f = rule(&r, id);
        assert_ne!(f.status, Status::Violated, "{id} caught an inversion arrow");
    }
}

// ------------------------------------------------------------- unevaluable

#[test]
fn a_rule_whose_premise_is_absent_is_never_a_pass() {
    // The single most misleading thing this tool could print is five
    // green checks against a crate with no ports.
    let Some(r) = sibling("arioch") else { return };
    for id in [
        "call-through-port",
        "port-signature-purity",
        "port-has-fake",
        "fat-trait",
        "concrete-outside-root",
        "domain-no-io",
    ] {
        let f = rule(&r, id);
        assert_eq!(f.status, Status::Unevaluable, "{id} reported {:?}", f.status);
        assert!(!f.because.is_empty(), "{id} does not say why");
    }
    assert!(r.unevaluable().len() > 10);
}

#[test]
fn mixed_layer_is_unevaluable_when_nothing_is_classified_to_compare() {
    // arioch classifies one symbol. A pass here would claim the modules
    // were examined.
    let Some(r) = sibling("arioch") else { return };
    assert_eq!(rule(&r, "mixed-layer-module").status, Status::Unevaluable);
}

// ------------------------------------------------------------ arioch

#[test]
fn arioch_global_mutable_state_is_a_violation_with_a_line_number() {
    // design-philosophy.md principle 3 names this one by hand.
    let Some(r) = sibling("arioch") else { return };
    let f = rule(&r, "no-global-mut");
    assert_eq!(f.status, Status::Violated);
    let i = f
        .instances
        .iter()
        .find(|i| i.detail.contains("CONFIG_OVERRIDE"))
        .expect("CONFIG_OVERRIDE not reported");
    assert_eq!(i.file, "config.rs");
    assert!(i.detail.contains("Mutex"));
}

#[test]
fn arioch_render_functions_read_the_registry_directly() {
    // view-model.md cites exactly this: `app.registry.entries` inside a
    // render fn. All four findings reach through the registry — a chain
    // ending in a method call is not an object-graph reach.
    let Some(r) = sibling("arioch") else { return };
    let f = rule(&r, "renders-off-store");
    assert_eq!(f.status, Status::Violated);
    assert!(f
        .instances
        .iter()
        .any(|i| i.detail.contains("app.registry.entries")));
    assert!(
        f.instances.iter().all(|i| i.detail.contains("app.registry")),
        "imprecise chains leaked in: {:?}",
        f.instances.iter().map(|i| &i.detail).collect::<Vec<_>>()
    );
}

#[test]
fn arioch_calls_the_clock_directly() {
    let Some(r) = sibling("arioch") else { return };
    let f = rule(&r, "time-injected");
    assert_eq!(f.status, Status::Violated);
    assert!(f.instances.iter().any(|i| i.to.contains("SystemTime::now")));
}

// ------------------------------------------------------- enforcement tier

#[test]
fn nothing_is_compiler_enforced_in_a_single_crate() {
    // The line that justifies the tool, and the number that will change.
    // `crate-graph.md` Option A is convention plus a source-scan test —
    // and this is that test.
    let r = fixture("ports");
    assert_eq!(r.compiler_enforced, 0);
    assert!(r.checked_here > 10);
}

#[test]
fn the_workspace_split_hands_three_rules_to_the_compiler() {
    // Under Option B the build rejects the import, so metatron reports
    // the rule as `compiler` and stops checking it. Reporting a metatron
    // pass there would overstate the tool's contribution.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/leaky");
    let m = metatron::scan(&dir).unwrap();
    let mut cfg = Config::load(&dir).unwrap();

    let a = check(&m, &cfg, &classify(&m, &cfg));
    assert_eq!(a.compiler_enforced, 0);
    assert_eq!(rule(&a, "domain-no-io").status, Status::Violated);

    cfg.crate_graph = "B".into();
    let b = check(&m, &cfg, &classify(&m, &cfg));
    assert_eq!(b.compiler_enforced, 3);
    let f = rule(&b, "domain-no-io");
    assert_eq!(f.tier, Tier::Compiler);
    assert_eq!(f.status, Status::Delegated);
    assert!(b.checked_here < a.checked_here);
}

#[test]
fn a_lint_the_toolchain_ships_is_listed_and_not_reimplemented() {
    let r = fixture("ports");
    let f = rule(&r, "panic-in-domain");
    assert_eq!(f.tier, Tier::Clippy);
    assert_eq!(f.status, Status::Delegated);
    assert!(f.because.contains("clippy"));
}
