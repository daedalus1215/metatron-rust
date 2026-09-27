//! Exit codes are a contract with CI, and this file is where it is written down:
//!
//! * `0` — the command ran and the answer is in the output.
//! * `1` — the command ran and the answer is "no".
//! * `2` — the command produced no answer at all.
//!
//! The distinction that matters is 1 against 2. A pipeline that reads `1` as
//! "architecture regressed" and `2` as "the tool is broken" cannot tell a real
//! regression from a typo in a path if the tool reports both as `1`.

use std::path::{Path, PathBuf};
use std::process::Command;

const NO_ANSWER: i32 = 2;
const REGRESSION: i32 = 1;

/// Every subcommand, so a new one cannot be added without a decision about
/// what it returns when it fails.
const COMMANDS: &[&str] = &["scan", "classify", "cohesion", "check", "views", "baseline"];

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_metatron"))
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("the metatron binary should be runnable")
}

/// A fixture copied into a temporary git repository with enough history for the
/// churn rule to have premises.
///
/// Spec 08 added `churn-concentration`, whose premise is git history, so a plain
/// fixture directory now always leaves one decidable rule unevaluable — which
/// makes `--require-evaluable` fail there for a reason that has nothing to do
/// with the flag. These tests are about the flag, so they get a crate that can
/// answer it. `OnceLock` because the repository is built once and the tests
/// that use it do not write to it.
fn repoed(fixture: &str) -> &'static Path {
    static DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture);
        let dst = std::env::temp_dir().join(format!("metatron-repoed-{fixture}"));
        let _ = std::fs::remove_dir_all(&dst);
        copy_tree(&src, &dst);

        let git = |args: &[&str]| {
            let ok = Command::new("git")
                .args(args)
                .current_dir(&dst)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            assert!(ok, "git {args:?} failed in {dst:?}");
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "t"]);
        git(&["add", "-A"]);
        // 25 commits: `Churn::MIN_COMMITS` is 20, and the rule says "not enough
        // history" below that, so a fixture with fewer would leave the very rule
        // these tests are about unevaluable.
        for i in 0..25 {
            // Under `src/`, because that is the model's root and therefore the
            // pathspec churn is measured against. Commits at the crate root
            // would leave the rule correctly reporting that nothing under
            // `src/` has ever changed.
            std::fs::write(
                dst.join("src").join(format!("history{i}.txt")),
                format!("commit {i}\n"),
            )
            .expect("write");
            git(&["add", "-A"]);
            git(&["commit", "-q", "-m", &format!("commit {i}")]);
        }
        dst
    })
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("mkdir");
    for e in std::fs::read_dir(from).expect("read_dir") {
        let e = e.expect("entry");
        let p = e.path();
        if p.is_dir() {
            copy_tree(&p, &to.join(e.file_name()));
        } else {
            std::fs::copy(&p, to.join(e.file_name())).expect("copy");
        }
    }
}

fn code(args: &[&str]) -> i32 {
    run(args)
        .status
        .code()
        .expect("the process should exit rather than be signalled")
}

#[test]
fn a_directory_that_is_not_there_is_not_a_regression() {
    for cmd in COMMANDS {
        assert_eq!(
            code(&[cmd, "no/such/directory"]),
            NO_ANSWER,
            "`metatron {cmd}` returned {} for a directory it never saw, so a CI \
             job cannot tell a broken build from a broken tool",
            code(&[cmd, "no/such/directory"])
        );
    }
}

#[test]
fn a_directory_that_is_not_there_says_so_on_stderr() {
    for cmd in COMMANDS {
        let out = run(&[cmd, "no/such/directory"]);
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(
            err.contains("metatron:") && err.contains("no/such/directory"),
            "`metatron {cmd}` failed without saying which directory it wanted: {err}"
        );
    }
}

#[test]
fn a_scan_that_ran_is_zero() {
    assert_eq!(code(&["scan", "--stdout", "tests/fixtures/dual"]), 0);
}

#[test]
fn the_json_and_text_paths_of_check_agree() {
    // `--json` used to `std::process::exit` from inside the print branch, and
    // the text path returned normally. Two exits for one verdict is how the
    // two paths drift apart; they are now one return value.
    let text = code(&["check", "."]);
    let json = code(&["check", "--json", "."]);
    assert_eq!(
        text, json,
        "`check` and `check --json` disagreed on the verdict"
    );
    assert!(
        text == 0 || text == REGRESSION,
        "a check that ran should answer 0 or {REGRESSION}, got {text}"
    );
}

#[test]
fn an_unrecognised_flag_is_not_a_regression() {
    // clap exits 2 on a usage error, which is the same meaning: nothing was
    // checked. Asserted so a future clap change cannot quietly make a typo
    // read as a failing architecture.
    assert_eq!(code(&["check", "--min-covrage", "50", "."]), NO_ANSWER);
}

#[test]
fn a_rule_name_that_gates_nothing_is_refused_not_passed() {
    // The failure this prevents: `gating on 0 rule(s)` and then PASS. A
    // misspelled rule name is the cheapest way to buy a green build, and it
    // is indistinguishable from a real pass unless the tool checks.
    let out = run(&["check", "--rule", "spec-99-no-such-rule", "."]);
    assert_eq!(
        out.status.code(),
        Some(NO_ANSWER),
        "a rule name that matched nothing was allowed to pass:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("spec-99-no-such-rule"), "{err}");
    // And it lists what it could have meant, which is the difference between
    // an error and a dead end.
    assert!(
        err.contains("the rules here are:"),
        "the error should name the real rules:\n{err}"
    );
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("PASS"),
        "a refused check printed a verdict"
    );
}

#[test]
fn an_advisory_rule_is_refused_for_its_own_reason() {
    // A heuristic rule is a real rule that cannot fail a build by design.
    // Asking for it by name is a different mistake from a typo, and the error
    // should say which one this is rather than insisting the rule is unknown.
    let out = run(&["check", "--rule", "fat-trait", "."]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(NO_ANSWER), "{err}");
    assert!(err.contains("advisory"), "{err}");
    assert!(
        !err.contains("matched no rule in this crate"),
        "a known rule was reported as unknown:\n{err}"
    );
}

#[test]
fn a_rule_name_that_gates_something_still_runs() {
    // The other half: the guard must not refuse a legitimate narrowing, or it
    // is just a new way to fail.
    let out = run(&["check", "--rule", "no-same-level", "."]);
    assert_ne!(
        out.status.code(),
        Some(NO_ANSWER),
        "a real gating rule was refused:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("gating on 1 rule(s)"),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn a_coverage_floor_is_a_gate_not_a_footnote() {
    // `gnarly` classifies 1 of its 5 symbols. Every rule here is blind to the
    // other four, so without a floor its PASS is nearly content-free.
    let out = run(&["check", "--min-coverage", "90", "tests/fixtures/gnarly"]);
    assert_eq!(
        out.status.code(),
        Some(REGRESSION),
        "a crate under the coverage floor passed:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("below the 90.0% floor"), "{stdout}");
    // It should say how much was invisible, not just that it was.
    assert!(
        stdout.contains("matched no pattern in metatron.toml"),
        "the failure should name the cause:\n{stdout}"
    );
    assert!(!stdout.contains("PASS"), "{stdout}");
}

#[test]
fn a_coverage_floor_below_the_actual_coverage_passes() {
    // Otherwise the flag is a switch that always fails, which is not a gate.
    assert_eq!(
        code(&["check", "--min-coverage", "10", "tests/fixtures/gnarly"]),
        0
    );
    // And a crate that classifies everything is not made to fail by a floor
    // it clears.
    assert_eq!(
        code(&["check", "--min-coverage", "90", "tests/fixtures/ports"]),
        0
    );
}

#[test]
fn the_coverage_floor_applies_to_the_json_path_too() {
    // `--json` returns before any of the narrative, so the floor has to be
    // applied inside it. A flag that works in one output mode and is ignored in
    // the other is the kind of half-feature CI depends on.
    assert_eq!(
        code(&[
            "check",
            "--json",
            "--min-coverage",
            "90",
            "tests/fixtures/gnarly"
        ]),
        REGRESSION
    );
    assert_eq!(
        code(&[
            "check",
            "--json",
            "--min-coverage",
            "10",
            "tests/fixtures/gnarly"
        ]),
        0
    );
}

#[test]
fn an_unevaluable_decidable_rule_is_a_gate_when_asked_for() {
    // `gnarly` leaves 14 decidable rules with no premises in the crate. They
    // are not passes; without the flag they are a caveat, with it they fail.
    let out = run(&["check", "--require-evaluable", "tests/fixtures/gnarly"]);
    assert_eq!(
        out.status.code(),
        Some(REGRESSION),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("could not be evaluated"), "{stdout}");
    // It has to name them, or "14 rules" is not actionable.
    assert!(stdout.contains("domain-no-io"), "{stdout}");
    assert!(!stdout.contains("PASS"), "{stdout}");
}

#[test]
fn a_heuristic_that_cannot_decide_does_not_trip_the_flag() {
    // A heuristic is honest about being unable to decide, and never gates. A
    // flag that failed on those would be unusable on any real crate, and would
    // push people to turn it off.
    let crate_path = repoed("ports");
    let out = run(&["check", "--require-evaluable", crate_path.to_str().unwrap()]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "a crate whose decidable rules all evaluated was failed:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn the_evaluable_flag_applies_to_the_json_path_too() {
    assert_eq!(
        code(&[
            "check",
            "--json",
            "--require-evaluable",
            "tests/fixtures/gnarly"
        ]),
        REGRESSION
    );
    assert_eq!(
        code(&[
            "check",
            "--json",
            "--require-evaluable",
            repoed("ports").to_str().unwrap()
        ]),
        0
    );
}

#[test]
fn a_mostly_unclassified_crate_names_its_worst_files() {
    // A percentage is a fact you cannot act on. The groups say where to open
    // the file, and name the config that would fix it.
    let out = run(&["check", "tests/fixtures/gnarly"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("matched no pattern"),
        "a crate 80% unclassified did not say so:\n{stdout}"
    );
    assert!(stdout.contains("main.rs"), "{stdout}");
    assert!(stdout.contains("metatron.toml"), "{stdout}");
    assert!(stdout.contains("e.g."), "no worked example:\n{stdout}");
}

#[test]
fn a_crate_under_the_threshold_is_not_scolded() {
    // Three stragglers in an otherwise-clean crate is a fact, not an
    // accusation. A warning that fires on nearly every crate is a warning
    // nobody reads, and this one would fire on every real project.
    let out = run(&["check", "tests/fixtures/ports"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        !stdout.contains("matched no pattern"),
        "a fully classified crate was warned about:\n{stdout}"
    );
}

#[test]
fn scan_names_the_unmatched_files_too() {
    // `scan` prints the coverage number too, so it owes the same explanation.
    // With --stdout there is no report at all, only the model, so that case
    // asserts the scan ran instead.
    let json = run(&["scan", "--stdout", "tests/fixtures/gnarly"]);
    let json = String::from_utf8_lossy(&json.stdout);
    assert!(json.starts_with('{'), "{}", &json[..json.len().min(80)]);

    let out = run(&["scan", "tests/fixtures/gnarly"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("matched no pattern") && stdout.contains("main.rs"),
        "{stdout}"
    );
}

#[test]
fn the_printed_exclusions_are_exactly_the_rules_that_do_not_gate() {
    // The scorecard's `enforcement` line, the baseline's exclusion list and the
    // "outside the ratchet" block under `check` are three renderings of one
    // predicate, and each was written separately. This reads the block the
    // binary actually printed and compares it, rule for rule, with `gates()`
    // evaluated in the library: a fifth call site that filtered on `kind`, or on
    // `gate` alone, or on `status == Violated`, would show up here as a name
    // that is on one side and not the other.
    for fixture in ["leaky", "mixed", "gnarly", "panics"] {
        let out = run(&["check", &format!("tests/fixtures/{fixture}")]);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let block = stdout
            .split("outside the ratchet")
            .nth(1)
            .unwrap_or_else(|| panic!("{fixture}: no ratchet block\n{stdout}"));
        let printed: Vec<&str> = block
            .lines()
            .skip(1)
            .take_while(|l| l.starts_with("    "))
            .filter_map(|l| l.split_whitespace().next())
            .collect();

        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture);
        let report = metatron::scan(&dir).unwrap();
        let cfg = metatron::classify::Config::load(&dir).unwrap();
        let classified = metatron::classify::classify(&report, &cfg);
        let churn = metatron::churn::Churn::measure(&dir, &cfg.root, &report);
        let checked = metatron::rules::check(&report, &cfg, &classified, &churn);

        let not_gating: Vec<&str> = checked
            .findings
            .iter()
            .filter(|f| !f.gates())
            .map(|f| f.id)
            .collect();
        assert_eq!(
            printed, not_gating,
            "{fixture}: the ratchet block and gates() name different rules"
        );
    }
}
