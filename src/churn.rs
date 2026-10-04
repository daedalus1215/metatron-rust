//! Churn: how often git says a file changed, against how much leans on it.
//!
//! This lives here rather than in `views/hotspots.rs` because two callers need
//! it and one of them is a rule. A number that decides where refactoring pays
//! for itself — `commits * (1 + dependents)`, computed for years in
//! `templates/hotspots.html` as JavaScript — belongs in the same place as the
//! data it ranks, or the chart and the build disagree and only one of them is in
//! the report.
//!
//! Everything here is read-only. It shells out to `git log` and writes nothing,
//! and a crate that is not in git at all is an answer rather than an error:
//! `available: false` with a `reason` saying which of the several absences
//! applied. An empty map with no reason is the failure mode spec 07 was written
//! about — a view that draws an empty scatter under a caption about churn, and
//! a caller that cannot tell an untracked crate from a broken tool.

use crate::model::Model;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

/// Per-file churn. Commits, lines moved, and who touched it.
#[derive(Serialize, Default, Clone, Debug)]
pub struct FileChurn {
    pub commits: usize,
    pub added: usize,
    pub removed: usize,
    pub first: String,
    pub last: String,
    #[serde(rename = "authorCount")]
    pub author_count: usize,
    /// How many files in the model import this one. Computed here, from the
    /// same edges the view draws, so the ranking and the picture cannot drift.
    pub deps: usize,
    /// `commits * (1 + deps)`, filled in by `Churn::add_dependents` and
    /// serialised for the template. Computed from [`score`] and never assigned
    /// by hand, so the field and the formula cannot drift into two opinions.
    pub score: usize,
}

/// The hotspot score: change frequency against how much leans on the file.
///
/// One function, because this used to exist twice — here and as
/// `c.commits * (1 + deps)` in `templates/hotspots.html` — and a rule and a
/// chart that disagree about which file is hot is worse than either alone.
pub fn score(commits: usize, deps: usize) -> usize {
    commits * (1 + deps)
}

/// The span the measurement covers, so a number can be read with its expiry.
#[derive(Serialize, Default, Clone, Debug)]
pub struct Window {
    /// Earliest commit date touching the root. The log is the whole history —
    /// there is no `--since` — so this is where the history starts, not a bound
    /// the caller chose.
    pub since: String,
    /// Distinct commits touching the root. Not the sum of the per-file counts:
    /// one commit that rewrites ten files is one commit, and adding the ten up
    /// produces a number that reads like history depth and is not. A repository
    /// of 26 commits can have 43 file touches, and printing the second one under
    /// the word "commits" is a number nobody can check.
    pub commits: usize,
    pub files: usize,
}

/// Churn for a crate: either data, or a stated reason there is none.
#[derive(Serialize, Default, Clone, Debug)]
pub struct Churn {
    pub files: BTreeMap<String, FileChurn>,
    pub window: Window,
    pub available: bool,
    /// Why not, when `available` is false. Never empty in that case, and never
    /// a bare "unavailable": the three absences below mean different things to
    /// whoever has to act on them.
    pub reason: String,
}

impl Churn {
    /// The stated absence, for a report that has to print something.
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Churn {
            available: false,
            reason: reason.into(),
            ..Default::default()
        }
    }

    /// Is this a repository with enough history to mean anything?
    ///
    /// Spec 08's minimum-history premise. A crate with four commits has no hot
    /// files; it has four commits, and a rule that says otherwise is measuring
    /// the author's first week.
    pub const MIN_COMMITS: usize = 20;

    pub fn has_history(&self) -> bool {
        self.available && !self.files.is_empty()
    }

    /// Enough history for a churn bound to be a statement about design rather
    /// than about the calendar.
    pub fn enough_history(&self) -> bool {
        self.has_history() && self.window.commits >= Self::MIN_COMMITS
    }

    pub fn total_commits(&self) -> usize {
        self.window.commits
    }

    /// Files that hold more than an even share of the window's churn and sit in
    /// the top decile of commits — the first half of
    /// `churn-concentration`.
    ///
    /// The even-share half is what stops this from reporting every file in a
    /// crate that changes uniformly. A repository of 56 commits spread over 43
    /// files has a top decile by definition, and one of those files held a single
    /// commit: a file with one commit in the window is not concentrating
    /// anything, and printing it beside a file with thirty-one is how a list of
    /// hotspots turns into noise. The even share is `1 / files`, so the question
    /// is answerable by anyone reading the number.
    ///
    /// Commits, not `score`: the score already contains the dependent count, and
    /// an axis that quietly includes the other one cannot be called independent.
    pub fn churn_hotspots(&self) -> BTreeSet<String> {
        let max = self.files.values().map(|c| c.commits).max().unwrap_or(0);
        if max == 0 {
            return BTreeSet::new();
        }
        let total: usize = self.files.values().map(|c| c.commits).sum();
        let n = self.files.len();
        self.files
            .iter()
            .filter(|(_, c)| c.commits * 10 >= max && c.commits * n > total)
            .map(|(f, _)| f.clone())
            .collect()
    }

    /// Files in the top decile of how many other files import them — the second
    /// half of `churn-concentration`. Empty when nothing is imported at all,
    /// which is a fact about the crate rather than a missing measurement.
    pub fn depended_upon(&self) -> BTreeSet<String> {
        let max = self.files.values().map(|c| c.deps).max().unwrap_or(0);
        if max == 0 {
            return BTreeSet::new();
        }
        self.files
            .iter()
            .filter(|(_, c)| c.deps * 10 >= max)
            .map(|(f, _)| f.clone())
            .collect()
    }

    /// Files in the top decile by score, ties included so a small crate with one
    /// busy file does not produce an empty decile.
    pub fn top_decile(&self) -> BTreeSet<String> {
        self.top_fraction(10)
    }

    /// Files at or above `1/n` of the maximum score.
    ///
    /// Relative, not absolute: an absolute commit count encodes this project's
    /// idea of a busy file, which is not a fact about the scanned crate. Ten
    /// commits in a two-week-old repo and ten commits in a two-year-old one are
    /// not the same observation.
    pub fn top_fraction(&self, n: usize) -> BTreeSet<String> {
        let max = self.files.values().map(|c| c.score).max().unwrap_or(0);
        if max == 0 {
            return BTreeSet::new();
        }
        self.files
            .iter()
            .filter(|(_, c)| c.score * n >= max)
            .map(|(f, _)| f.clone())
            .collect()
    }

    /// Does this path have any commit of its own? The cheap question the view
    /// asks before deciding whether it has a view to draw.
    pub fn has_commits(dir: &Path, root: &str) -> bool {
        Command::new("git")
            .args(["rev-list", "--count", "HEAD", "--", root])
            .current_dir(dir)
            .output()
            .map(|o| {
                o.status.success()
                    && String::from_utf8_lossy(&o.stdout)
                        .trim()
                        .parse::<usize>()
                        .is_ok_and(|n| n > 0)
            })
            .unwrap_or(false)
    }

    /// Measure `root` inside the git work tree at `dir`.
    ///
    /// `root` is the model's root-relative coordinate system, so the log is
    /// scoped the same way the model is: a workspace member's churn is its own,
    /// not its workspace's.
    pub fn measure(dir: &Path, root: &str, model: &Model) -> Churn {
        let (files, commits) = match Self::log(dir, root) {
            Ok(v) => v,
            Err(why) => return Churn::unavailable(why),
        };
        if files.is_empty() {
            return Churn::unavailable(format!("no commit in this repository touches `{root}`"));
        }
        let mut ch = Churn {
            files,
            ..Default::default()
        };

        ch.add_dependents(model);
        let since = ch
            .files
            .values()
            .map(|c| c.first.clone())
            .min()
            .unwrap_or_default();

        ch.window = Window {
            since,
            commits,
            files: ch.files.len(),
        };
        ch.available = true;
        ch
    }

    /// `git log --no-merges --numstat`, parsed. A missing or empty history is
    /// not an error — a crate does not have to be in git to be analysed — but it
    /// is a reason, and which reason is decided here rather than by an empty map
    /// downstream.
    ///
    /// Also returns the number of commits in the log, counted once each. The
    /// `--numstat` body lists a line per file per commit, so a sum of those lines
    /// counts a wide commit several times; the hashes are right there in the
    /// format string and this is the only place that knows the difference.
    #[allow(clippy::type_complexity)]
    fn log(dir: &Path, root: &str) -> Result<(BTreeMap<String, FileChurn>, usize), String> {
        let out = Command::new("git")
            .args([
                "log",
                "--no-merges",
                "--numstat",
                "--format=%x01%H%x01%an%x01%ad",
                "--date=short",
                "--",
                root,
            ])
            .current_dir(dir)
            .output()
            .map_err(|e| format!("git could not be run: {e}"))?;

        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            let err = err.lines().next().unwrap_or("").trim();
            return Err(if err.is_empty() {
                format!("`{}` is not inside a git work tree", dir.display())
            } else {
                format!("git log failed: {err}")
            });
        }

        let text = String::from_utf8_lossy(&out.stdout);
        let mut files: BTreeMap<String, FileChurn> = BTreeMap::new();
        let mut authors: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let (mut author, mut date) = (String::new(), String::new());
        let mut commits: BTreeSet<String> = BTreeSet::new();

        for line in text.lines() {
            if let Some(rest) = line.strip_prefix('\u{1}') {
                let mut p = rest.split('\u{1}');
                if let Some(hash) = p.next() {
                    commits.insert(hash.to_string());
                }
                author = p.next().unwrap_or("").to_string();
                date = p.next().unwrap_or("").to_string();
                continue;
            }
            let mut f = line.split('\t');
            let (Some(a), Some(r), Some(path)) = (f.next(), f.next(), f.next()) else {
                continue;
            };
            // A binary file shows `-` for both counts.
            let (a, r) = (
                a.parse::<usize>().unwrap_or(0),
                r.parse::<usize>().unwrap_or(0),
            );
            let rel = path
                .strip_prefix(&format!("{root}/"))
                .unwrap_or(path)
                .to_string();
            let e = files.entry(rel.clone()).or_default();
            e.commits += 1;
            e.added += a;
            e.removed += r;
            if e.last.is_empty() {
                e.last = date.clone(); // git log is newest-first
            }
            e.first = date.clone();
            authors.entry(rel).or_default().insert(author.clone());
        }
        for (f, set) in authors {
            if let Some(e) = files.get_mut(&f) {
                e.author_count = set.len();
            }
        }
        Ok((files, commits.len()))
    }

    /// Count, per file, how many other files import it.
    ///
    /// The view derived this from `fileLinks` in JavaScript, keyed by module
    /// index. Doing it here means the rule and the chart count the same
    /// edges — and `Model::file_links` is the one definition of "imports" in
    /// this codebase, so a third notion cannot creep in beside it.
    fn add_dependents(&mut self, model: &Model) {
        let by_module: BTreeMap<&str, &str> = model
            .modules
            .iter()
            .map(|m| (m.id.as_str(), m.file.as_str()))
            .collect();
        let mut deps: BTreeMap<String, usize> = BTreeMap::new();
        for (from, to) in model.file_links() {
            let (Some(a), Some(b)) = (by_module.get(from.as_str()), by_module.get(to.as_str()))
            else {
                continue;
            };
            *deps.entry((*b).to_string()).or_default() += 1;
            let _ = a;
        }
        for (file, c) in self.files.iter_mut() {
            c.deps = deps.get(file).copied().unwrap_or(0);
            c.score = score(c.commits, c.deps);
        }
    }
}
