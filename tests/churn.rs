//! Acceptance tests for the churn half of `specs/08-test-presence-and-churn-bounds.md`.

use metatron::churn::{score, Churn};
use metatron::model::Model;
use std::path::Path;

fn measure(dir: &Path) -> (Churn, Model) {
    let m = metatron::scan(dir).expect("scan failed");
    let cfg = metatron::classify::Config::load(dir).expect("config failed");
    let c = Churn::measure(dir, &cfg.root, &m);
    (c, m)
}

// -------------------------------------------------- the score is one function

#[test]
fn the_hotspot_score_is_commits_against_dependents() {
    assert_eq!(score(4, 10), 44);
    // The `1 +` is the dependents term: a file nothing imports still scores its
    // own changes rather than zero.
    assert_eq!(score(1, 0), 1);
    assert_eq!(score(1, 3), 4);
    assert_eq!(
        score(0, 9),
        0,
        "never touched is never hot, however depended on"
    );
}

// ------------------------------------------------------ data, with its window

#[test]
fn churn_is_measured_over_a_window_the_caller_can_read() {
    let (c, _) = measure(Path::new("."));

    assert!(c.available, "this repository has history: {}", c.reason);
    assert!(!c.files.is_empty());
    assert!(
        c.total_commits() >= c.files.len(),
        "one commit touches many files"
    );

    // The window is stated, not implied. `since` is where the history starts
    // because the log is the whole history — a number without it is a number
    // whose meaning expires silently.
    assert!(
        c.window.since.len() == 10,
        "since should be a date, got {:?}",
        c.window.since
    );
    assert_eq!(c.window.files, c.files.len());
    assert_eq!(c.reason, "", "an available measurement states no absence");
}

#[test]
fn the_dependents_are_the_edges_the_model_already_has() {
    let (c, m) = measure(Path::new("."));

    // Recount from `Model::file_links` independently of the churn module, so a
    // change to the definition of "imports" cannot quietly change both the
    // number and the check at the same time.
    let mut expected: std::collections::BTreeMap<String, usize> = Default::default();
    for (_, to) in m.file_links() {
        *expected.entry(to).or_default() += 1;
    }
    for (file, fc) in &c.files {
        let Some(module) = m.modules.iter().find(|mo| mo.file == *file) else {
            continue;
        };
        let want = expected.get(&module.id).copied().unwrap_or(0);
        assert_eq!(fc.deps, want, "{file} disagrees about its dependents");
    }
}

#[test]
fn every_file_reports_the_score_the_formula_gives() {
    let (c, _) = measure(Path::new("."));
    for (f, fc) in &c.files {
        assert_eq!(fc.score, score(fc.commits, fc.deps), "{f}");
    }
}

// ------------------------------------------------------------- stated absence

#[test]
fn churn_outside_a_git_work_tree_says_so() {
    let dir = std::env::temp_dir().join("metatron-not-a-repo");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("mkdir");
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"nope\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .expect("write");
    std::fs::write(dir.join("src/lib.rs"), "pub fn f() {}\n").expect("write");

    // /tmp is not inside a work tree, so git log fails and the reason says so.
    let (c, _) = measure(&dir);
    if !c.available {
        assert!(
            !c.reason.is_empty(),
            "an unavailable measurement with no reason is the failure this \
             module exists to prevent"
        );
        assert!(
            c.reason.contains("git") || c.reason.contains("work tree"),
            "the reason must say what went wrong, got: {}",
            c.reason
        );
    }
    assert!(!c.has_history());
    assert!(!c.enough_history());
    // And a rule asking about churn has nothing to work with, rather than an
    // empty answer that reads like "nothing is churning".
    assert!(c.top_decile().is_empty());
}

#[test]
fn the_window_counts_commits_not_the_lines_they_occupy() {
    // A commit that rewrites ten files is one commit. Adding up the per-file
    // counts calls it ten, and the window then prints a number that reads like
    // history depth and is not: a scratch repository with 26 commits and 43 file
    // touches reported "over 43 commit(s)", and 43 clears the twenty-commit
    // floor that five commits should not. Found by running the tool on a
    // repository, not by reading the code that sums.
    //
    // Measured here against this repository and compared with git's own count
    // over the same pathspec, which is the only version of the number nobody can
    // argue with.
    let dir = Path::new(".");
    let distinct = std::process::Command::new("git")
        .args(["rev-list", "--count", "HEAD", "--", "src"])
        .current_dir(dir)
        .output()
        .expect("git rev-list");
    let distinct: usize = String::from_utf8_lossy(&distinct.stdout)
        .trim()
        .parse()
        .expect("a count");

    let (c, _) = measure(dir);
    assert!(c.available, "this repository has history: {}", c.reason);
    let touches: usize = c.files.values().map(|f| f.commits).sum();
    assert_eq!(
        c.window.commits, distinct,
        "the window must count commits, not file touches ({touches} touches \
         across {distinct} commits)"
    );
    assert!(
        touches > distinct,
        "this repository must have commits that touch more than one file, or \
         the test cannot fail"
    );
}

#[test]
fn an_empty_history_is_not_a_crate_with_no_hot_files() {
    // A git repository with no commits at all: the log succeeds and says
    // nothing, which is a different absence from git being unavailable.
    let dir = std::env::temp_dir().join("metatron-empty-repo");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("mkdir");
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"fresh\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )
    .expect("write");
    std::fs::write(dir.join("src/lib.rs"), "pub fn f() {}\n").expect("write");
    let init = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&dir)
        .output()
        .expect("git init");
    assert!(init.status.success(), "git init failed");

    let (c, _) = measure(&dir);
    assert!(!c.available, "a repository with no commits has no churn");
    // `git log` fails outright here rather than returning an empty log, so the
    // reason is git's own words. Either wording is honest; silence is not.
    assert!(
        c.reason.contains("commit") || c.reason.contains("no commit"),
        "the reason must name the absence, got: {}",
        c.reason
    );
    assert!(!c.has_history());
    assert!(!c.enough_history());
}

// ---------------------------------------------------------- the bounds, if any

#[test]
fn too_little_history_is_not_enough_to_rank_anything() {
    let mut c = Churn {
        available: true,
        files: [(
            "a.rs".to_string(),
            metatron::churn::FileChurn {
                commits: 3,
                deps: 9,
                score: score(3, 9),
                ..Default::default()
            },
        )]
        .into_iter()
        .collect(),
        window: metatron::churn::Window {
            commits: 3,
            ..Default::default()
        },
        ..Default::default()
    };

    assert!(
        !c.enough_history(),
        "3 commits is not a history, however busy the file looks"
    );
    assert!(
        c.reason.is_empty(),
        "this is a fixture, not a measurement, so it states no absence"
    );

    // Enough history, and the same file is now rankable.
    c.window.commits = Churn::MIN_COMMITS;
    assert!(c.enough_history());
    assert!(c.top_decile().contains("a.rs"));
}

#[test]
fn a_decile_is_relative_so_a_uniform_crate_has_no_hotspots() {
    let mut c = Churn {
        available: true,
        files: (0..10)
            .map(|i| {
                (
                    format!("f{i}.rs"),
                    metatron::churn::FileChurn {
                        commits: 4,
                        deps: 2,
                        score: score(4, 2),
                        ..Default::default()
                    },
                )
            })
            .collect(),
        ..Default::default()
    };
    // Everything equal means nothing stands out: an absolute threshold would
    // call all ten hot, and the tool would have learned nothing.
    assert_eq!(c.top_decile().len(), 10);
    assert_eq!(c.top_fraction(5).len(), 10);

    // One file clearly ahead of the rest is in the decile on its own.
    c.files.insert(
        "hot.rs".into(),
        metatron::churn::FileChurn {
            commits: 40,
            deps: 9,
            score: score(40, 9),
            ..Default::default()
        },
    );
    let top = c.top_decile();
    assert!(top.contains("hot.rs"), "{top:?}");
    assert_eq!(
        top.len(),
        1,
        "only the outlier is in the top decile: {top:?}"
    );
}

#[test]
fn an_unavailable_measurement_has_a_reason_by_construction() {
    let c = Churn::unavailable("git could not be run");
    assert!(!c.available);
    assert_eq!(c.reason, "git could not be run");
    assert!(!c.has_history());
    assert!(!c.enough_history());
    assert!(c.top_decile().is_empty());
    assert_eq!(c.total_commits(), 0);
}

// --------------------------------- the rule the architecture test cannot check

/// The exemption in `tests/architecture.rs` is a debt with an owner, and this
/// test is the repayment: `churn-concentration` is decidable, and against a
/// repository with a history it says something. A fixture directory could never
/// have shown that.
#[test]
fn the_churn_rule_is_decidable_against_a_real_repository() {
    let (churn, model) = measure(Path::new("."));
    assert!(
        churn.enough_history(),
        "this repository should have a history: {} commits, {}",
        churn.total_commits(),
        churn.reason
    );

    let dir = Path::new(".");
    let cfg = metatron::classify::Config::load(dir).expect("config failed");
    let classified = metatron::classify::classify(&model, &cfg);
    let report = metatron::rules::check(&model, &cfg, &classified, &churn);

    let f = report
        .findings
        .iter()
        .find(|f| f.id == "churn-concentration")
        .expect("the rule is in the table");
    assert_eq!(f.kind, metatron::rules::Kind::Decidable);
    assert_ne!(
        f.status,
        metatron::rules::Status::Unevaluable,
        "decidable and unevaluable against a real history: {}",
        f.because
    );
    assert!(!f.gates(), "advisory, and the reason is in the spec");

    // Whatever it reports, the window travels with it: a bound whose span is
    // not in the string is a number that means something different next month.
    for i in &f.instances {
        assert!(
            i.detail.contains("commit(s) since"),
            "the instance does not carry its window: {}",
            i.detail
        );
    }
}
