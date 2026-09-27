//! The docs are part of the tool.
//!
//! A README that shows output the binary does not produce, or a link to a file
//! that was never in the repository, is the same defect as a rule with a reason
//! string citing a file that does not exist: a claim the reader cannot check and
//! that turns out to be false. Spec 07 §"The tool passes its own hygiene" asks
//! for both to be caught mechanically, so this file is where they are.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    let p = repo().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

#[test]
fn every_relative_link_resolves() {
    // Checked over the README, the spec set and the tools README. A link to a
    // sibling checkout is fine — `metatron-nestjs` lives next to this one — and
    // so is anything off the web. A link to a path inside this repository that
    // is not there is a promise the repository cannot keep, and there were two:
    // `patterns-rust`, which is the layout convention the specs are written
    // against rather than a checkout beside this one.
    let mut checked = 0;
    let scanned = docs();
    assert!(
        scanned.len() >= 8,
        "only {} docs scanned: the link check is looking at almost nothing",
        scanned.len()
    );
    for doc in &scanned {
        for target in links(&read(doc)) {
            let Some(p) = local_target(&repo().join(doc), &target) else {
                continue;
            };
            assert!(
                p.exists(),
                "{doc} links to {target}, which does not exist (looked in {})",
                p.display()
            );
            checked += 1;
        }
    }
    assert!(
        checked > 0,
        "no local links at all: the check found nothing"
    );
}

/// Every markdown file whose links are a promise: the two READMEs and the specs.
fn docs() -> Vec<String> {
    let mut v = vec!["README.md".to_string(), "tools/README.md".to_string()];
    v.extend(
        sorted_specs()
            .into_iter()
            .filter(|s| !s.ends_with("README.md")),
    );
    v
}

fn sorted_specs() -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(repo().join("specs"))
        .expect("specs/")
        .filter_map(|e| {
            let p = e.ok()?.path();
            (p.extension()? == "md").then(|| p.display().to_string())
        })
        .collect();
    v.sort();
    v
}

/// Every markdown link target, with the fragment and any title removed.
fn links(md: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = md;
    while let Some(at) = rest.find("](") {
        rest = &rest[at + 2..];
        let Some(end) = rest.find(')') else { break };
        let target = rest[..end].split_whitespace().next().unwrap_or("");
        out.push(target.to_string());
        rest = &rest[end..];
    }
    out
}

/// The path a link points at, or `None` for a URL, an anchor, or a bare word.
fn local_target(from: &Path, target: &str) -> Option<PathBuf> {
    if target.is_empty()
        || target.starts_with('#')
        || target.contains("://")
        || target.starts_with("mailto:")
    {
        return None;
    }
    let path = target.split('#').next().unwrap_or(target);
    Some(from.parent()?.join(path))
}

#[test]
fn the_sample_scan_output_is_a_real_run() {
    // The block under "Try it" is the first thing a reader copies, and it was
    // missing the coverage line — the number that says the classifier is doing
    // almost nothing on this crate, which is the most important line in the
    // output. Asserted as a contiguous excerpt of a real run rather than as
    // whole output, because a README that quotes twenty-two lines of a stranger's
    // terminal is a README nobody reads to the end.
    //
    // Skipped without the arioch checkout, like the other cross-repository
    // assertions: the sample names a crate that is not in this repository.
    let arioch = repo().join("../arioch");
    if !arioch.join("Cargo.toml").exists() {
        return;
    }
    let bin = repo().join("target/debug/metatron");
    if !bin.exists() {
        return;
    }
    let out = Command::new(&bin)
        .args(["scan", arioch.to_str().unwrap()])
        .current_dir(repo())
        .output()
        .expect("the binary should run");
    let real = String::from_utf8_lossy(&out.stdout);
    let sample = scan_sample();
    assert!(
        real.contains(&sample),
        "the README's sample is not a run of this binary.\n\
         first line that does not appear: {}",
        sample
            .lines()
            .find(|l| !real.contains(l))
            .unwrap_or("(none — the sample is a subset, not contiguous)")
    );
    assert!(
        sample.contains("coverage"),
        "the sample omits the coverage line"
    );
}

/// The fenced block in README.md that starts with `metatron scan`.
fn scan_sample() -> String {
    let md = read("README.md");
    let start = md
        .find("```\nmetatron scan")
        .expect("README has no sample scan output")
        + 4;
    let end = md[start..].find("```").expect("unterminated block") + start;
    md[start..end].trim_end().to_string()
}

#[test]
fn the_status_table_lists_every_spec_that_exists() {
    // The table is how a reader decides whether to trust the rest. A spec file
    // with no row reads as work nobody has looked at, which is the state this
    // repository was in for a week.
    let md = read("README.md");
    for s in sorted_specs()
        .into_iter()
        .filter(|s| !s.ends_with("README.md"))
    {
        let n = Path::new(&s)
            .file_stem()
            .and_then(|x| x.to_str())
            .unwrap()
            .split('-')
            .next()
            .unwrap()
            .to_string();
        assert!(
            md.contains(&format!("| {n} ")),
            "spec {n} exists as {s} and has no row in the README status table"
        );
    }
}
