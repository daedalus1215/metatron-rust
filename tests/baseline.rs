//! Acceptance tests for `specs/05-scorecard-and-baseline.md`.

use metatron::baseline::{self, Baseline};
use metatron::rules::Kind;
use std::path::{Path, PathBuf};

/// Copy a fixture somewhere writable. These tests mutate source and write
/// baselines, and doing that in `tests/fixtures/` would leave the repo
/// dirty and make the tests order-dependent.
struct Sandbox(PathBuf);

impl Sandbox {
    fn of(fixture: &str, tag: &str) -> Self {
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture);
        let dst = std::env::temp_dir().join(format!("metatron-{fixture}-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dst);
        copy(&src, &dst);
        Sandbox(dst)
    }
    fn path(&self) -> &Path {
        &self.0
    }
    fn edit(&self, rel: &str, from: &str, to: &str) {
        let p = self.0.join(rel);
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains(from), "`{from}` not in {rel}");
        std::fs::write(&p, s.replace(from, to)).unwrap();
    }
    fn append(&self, rel: &str, text: &str) {
        let p = self.0.join(rel);
        let mut s = std::fs::read_to_string(&p).unwrap();
        s.push_str(text);
        std::fs::write(&p, s).unwrap();
    }
    fn accept(&self) -> Baseline {
        let s = metatron::check(self.path());
        let (next, _) = baseline::update(&s.report, &s.baseline, &s.model.project, s.coverage());
        next.save(self.path()).unwrap();
        next
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn copy(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), to).unwrap();
        }
    }
}

fn tree(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let e = e.unwrap();
            if e.file_type().unwrap().is_dir() {
                stack.push(e.path());
            }
            out.push(e.path().display().to_string());
        }
    }
    out.sort();
    out
}

// ------------------------------------------------------------ fingerprints

#[test]
fn the_fingerprint_is_stable_and_names_the_offender() {
    let a = baseline::fingerprint("domain-no-io", "domain::x", "std::fs");
    assert_eq!(a.len(), 12);
    assert_eq!(a, baseline::fingerprint("domain-no-io", "domain::x", "std::fs"));
    // Any of the three parts changing is a different violation.
    assert_ne!(a, baseline::fingerprint("domain-no-io", "domain::y", "std::fs"));
    assert_ne!(a, baseline::fingerprint("domain-no-io", "domain::x", "std::process"));
    assert_ne!(a, baseline::fingerprint("time-injected", "domain::x", "std::fs"));
}

#[test]
fn a_swap_is_caught_where_a_count_would_pass() {
    // One violation fixed and another introduced in the same change. The
    // total is unchanged, so a per-rule count nets it to zero and passes.
    // This is the whole argument for fingerprinting.
    let s = Sandbox::of("leaky", "swap");
    s.accept();
    let before = metatron::check(s.path());
    let total_before = before.diff.new.len() + before.diff.known.len();
    assert_eq!(before.diff.new.len(), 0);

    s.edit(
        "src/domain/mod.rs",
        "pub struct ActivityDto { pub name: String }",
        "pub struct ActivityName { pub name: String }",
    );
    s.append(
        "src/domain/mod.rs",
        "\n#[derive(Clone, PartialEq)]\npub struct SessionDto { pub id: i64 }\n",
    );

    let after = metatron::check(s.path());
    assert_eq!(
        after.diff.new.len() + after.diff.known.len(),
        total_before,
        "the swap should leave the total unchanged"
    );
    assert_eq!(after.diff.new.len(), 1, "the new violation was missed");
    assert_eq!(after.diff.fixed.len(), 1, "the fixed violation was missed");
    assert_eq!(after.exit_code(0), 1);
}

// ---------------------------------------------------------------- ratchet

#[test]
fn accepting_the_baseline_makes_an_unchanged_tree_pass() {
    let s = Sandbox::of("leaky", "accept");
    let before = metatron::check(s.path());
    assert!(before.exit_code(0) == 1, "a crate built to break the rules should fail first");

    s.accept();
    let after = metatron::check(s.path());
    assert_eq!(after.diff.new.len(), 0);
    assert_eq!(after.diff.fixed.len(), 0);
    assert_eq!(after.exit_code(0), 0);
    after.assert_no_new_violations();
}

#[test]
fn one_new_import_fails_and_names_the_file_and_line() {
    let s = Sandbox::of("ports", "regress");
    s.accept();
    metatron::check(s.path()).assert_no_new_violations();

    s.append(
        "src/domain/use_cases/activity.rs",
        "\npub fn leak_activity() { let _ = rusqlite::Connection::open(\"x\"); }\n",
    );

    let after = metatron::check(s.path());
    assert_eq!(after.exit_code(0), 1);
    let n = after.new_gating();
    assert!(!n.is_empty());
    let f = after
        .report
        .findings
        .iter()
        .find(|f| f.id == "domain-no-io")
        .unwrap();
    let i = f
        .instances
        .iter()
        .find(|i| i.from.contains("leak_activity"))
        .expect("the new import was not located");
    assert_eq!(i.file, "domain/use_cases/activity.rs");
    assert!(i.line > 0);
}

#[test]
fn a_fixed_violation_is_reported_and_the_baseline_is_not_mutated() {
    // A scan that temporarily fails to parse a file would otherwise
    // quietly retire a real debt, which returns later as a "new"
    // violation with no history.
    let s = Sandbox::of("leaky", "fixed");
    s.accept();
    let before = std::fs::read_to_string(baseline::path_of(s.path())).unwrap();

    s.edit(
        "src/domain/mod.rs",
        "pub struct ActivityDto { pub name: String }",
        "pub struct ActivityName { pub name: String }",
    );

    let after = metatron::check(s.path());
    assert_eq!(after.diff.fixed.len(), 1);
    assert_eq!(after.exit_code(0), 0, "fixing something must not fail the build");

    let now = std::fs::read_to_string(baseline::path_of(s.path())).unwrap();
    assert_eq!(before, now, "check mutated the baseline");
}

#[test]
fn allow_new_tolerates_exactly_what_it_says() {
    let s = Sandbox::of("leaky", "allow");
    s.accept();
    s.append(
        "src/domain/converters.rs",
        "\npub fn to_thing(a: &Activity) -> String {\n \
         let _ = std::process::Command::new(\"ls\");\n a.name.clone()\n}\n",
    );
    let after = metatron::check(s.path());
    let n = after.new_gating().len();
    assert!(n >= 1);
    assert_eq!(after.exit_code(n - 1), 1);
    assert_eq!(after.exit_code(n), 0);
}

// ------------------------------------------------------------------ notes

#[test]
fn hand_written_notes_survive_an_update_and_dropped_ones_are_counted() {
    // metatron-nestjs shipped a bug here: `--update` reported "1 note
    // preserved" while preserving none.
    let s = Sandbox::of("leaky", "notes");
    let mut b = s.accept();

    let keep = b
        .violations
        .iter()
        .find(|(_, e)| e.rule == "no-global-mut")
        .map(|(k, _)| k.clone())
        .unwrap();
    let lose = b
        .violations
        .iter()
        .find(|(_, e)| e.rule == "dto-in-domain")
        .map(|(k, _)| k.clone())
        .unwrap();
    b.violations.get_mut(&keep).unwrap().note = "blocked on the Config port".into();
    b.violations.get_mut(&lose).unwrap().note = "renamed next commit".into();
    b.save(s.path()).unwrap();

    // Fix the one carrying the note that must be dropped.
    s.edit(
        "src/domain/mod.rs",
        "pub struct ActivityDto { pub name: String }",
        "pub struct ActivityName { pub name: String }",
    );

    let sc = metatron::check(s.path());
    let (next, out) =
        baseline::update(&sc.report, &sc.baseline, &sc.model.project, sc.coverage());

    assert_eq!(out.notes_preserved, 1, "the surviving note was not preserved");
    assert_eq!(out.notes_dropped.len(), 1, "the dropped note was not reported");
    assert_eq!(out.notes_dropped[0].0, lose);
    assert_eq!(out.notes_dropped[0].1.note, "renamed next commit");
    assert_eq!(next.violations[&keep].note, "blocked on the Config port");
    assert!(!next.violations.contains_key(&lose));
}

#[test]
fn a_known_violation_carries_its_note_into_the_report() {
    let s = Sandbox::of("leaky", "carry");
    let mut b = s.accept();
    let k = b
        .violations
        .iter()
        .find(|(_, e)| e.rule == "no-global-mut")
        .map(|(k, _)| k.clone())
        .unwrap();
    b.violations.get_mut(&k).unwrap().note = "deliberate, see ADR-4".into();
    b.save(s.path()).unwrap();

    let sc = metatron::check(s.path());
    let known = sc.diff.known.iter().find(|(fp, _)| *fp == k).unwrap();
    assert_eq!(known.1.note, "deliberate, see ADR-4");
}

// ------------------------------------------------------------- heuristics

#[test]
fn no_heuristic_can_enter_the_baseline_or_change_the_exit_code() {
    // By construction, not by configuration. There is no flag that lets a
    // guess fail a build.
    let s = Sandbox::of("leaky", "heur");
    let sc = metatron::check(s.path());

    let heuristics: Vec<&str> = sc
        .report
        .findings
        .iter()
        .filter(|f| f.kind == Kind::Heuristic)
        .map(|f| f.id)
        .collect();
    assert!(heuristics.len() >= 5);

    let (cur, excluded) = baseline::current(&sc.report);
    for h in &heuristics {
        assert!(
            !cur.values().any(|e| e.rule == *h),
            "{h} entered the ratchet"
        );
        assert!(excluded.iter().any(|x| x.rule == *h), "{h} was excluded silently");
    }
    // And every exclusion says why, so a rule cannot sit outside the gate
    // unnoticed.
    assert!(excluded.iter().all(|x| !x.why.is_empty()));
    assert!(!sc.gating_rules().iter().any(|r| heuristics.contains(r)));
}

// ------------------------------------------------------------ the test API

#[test]
fn the_library_path_writes_nothing_to_disk() {
    // `metatron::check(".")` runs inside someone's `cargo test`. A test
    // that mutates the working tree is a bad test.
    let s = Sandbox::of("ports", "readonly");
    let before = tree(s.path());
    let sc = metatron::check(s.path());
    let _ = metatron::cohesion(s.path());
    let after = tree(s.path());
    assert_eq!(before, after, "the library path touched the tree");
    assert!(!baseline::Baseline::exists(s.path()));
    assert!(!s.path().join(".metatron").exists());
    let _ = sc.counts();
}

#[test]
fn the_panic_message_is_the_report() {
    // `cargo test` output is the only thing anyone will read.
    let s = Sandbox::of("leaky", "panic");
    let sc = metatron::check(s.path());
    let msg = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        sc.assert_no_new_violations()
    }))
    .unwrap_err();
    let text = msg
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_else(|| "not a string".into());

    assert!(text.contains("new architecture violation"), "{text}");
    assert!(text.contains("domain-no-io"), "{text}");
    assert!(text.contains(".rs:"), "no file:line in the message: {text}");
    assert!(text.contains("fingerprint"), "{text}");
    assert!(text.contains("metatron baseline"), "no next step offered: {text}");
}

#[test]
fn cohesion_can_be_locked_in_for_one_named_type() {
    // Spec 02 declines to gate cohesion globally, and still does. A
    // threshold the author chose for one named type, in their own test
    // file, is a different thing.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cohesion");
    let c = metatron::cohesion(&dir);
    c.assert_max_components("Split", 2);

    let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        c.assert_max_components("Split", 1)
    }))
    .unwrap_err();
    let text = err.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(text.contains("2 cohesion component(s)"), "{text}");
    assert!(text.contains("modularity"), "{text}");

    // A type that is not analysed says so rather than passing vacuously.
    let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        c.assert_max_components("NoSuchType", 1)
    }))
    .unwrap_err();
    let text = err.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(text.contains("no type named"), "{text}");
}

// ------------------------------------------------------------- the target

#[test]
fn arioch_scores_as_a_crate_with_no_architecture_yet() {
    // A run that reports a healthy architecture for arioch is a failed
    // acceptance test.
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../arioch");
    if !p.join("Cargo.toml").exists() {
        return;
    }
    let s = metatron::check(&p);
    let c = s.counts();
    let e = s.enforcement();

    assert!(s.coverage() < 5.0, "coverage {:.1}%", s.coverage());
    assert_eq!(e.compiler, 0, "nothing is compiler-enforced in a single crate");
    assert!(c.unevaluable > 10, "unevaluable {}", c.unevaluable);
    assert!(c.upheld == 0, "arioch upholds no port");
    assert!(c.violated > 0);

    // And the shape of the scorecard itself: no single percentage.
    assert!(!s.had_baseline, "arioch has no committed baseline");

    let err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        s.assert_no_unevaluable()
    }))
    .unwrap_err();
    let text = err.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(text.contains("they are not passing"), "{text}");
}
