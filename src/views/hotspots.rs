//! `hotspots` — git churn against dependents. Entirely language-agnostic,
//! and the only view that produces a real result on a crate with no
//! architecture yet: a file that changes constantly and that everything
//! depends on is a problem in any language.

use super::{stats, tier_index, tiers, Stats, Tier};
use crate::scorecard::Scorecard;
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

#[derive(Serialize, Default, Clone)]
pub struct Churn {
    pub commits: usize,
    pub added: usize,
    pub removed: usize,
    pub first: String,
    pub last: String,
    #[serde(rename = "authorCount")]
    pub author_count: usize,
}

#[derive(Serialize)]
pub struct FileNode {
    pub f: String,
    pub m: String,
    pub p: String,
    pub t: usize,
    pub cls: String,
    pub loc: u32,
}

#[derive(Serialize)]
pub struct ChurnMeta {
    pub since: String,
    pub commits: usize,
    pub files: usize,
    pub available: bool,
}

#[derive(Serialize)]
pub struct Hotspots {
    #[serde(rename = "generatedAt")]
    pub generated_at: String,
    pub project: String,
    pub root: String,
    pub stats: Stats,
    pub coverage: f64,
    pub tiers: Vec<Tier>,
    #[serde(rename = "fileNodes")]
    pub file_nodes: Vec<FileNode>,
    #[serde(rename = "fileLinks")]
    pub file_links: Vec<(String, String)>,
    pub churn: BTreeMap<String, Churn>,
    #[serde(rename = "churnMeta")]
    pub churn_meta: ChurnMeta,
    pub findings: Vec<super::city::CityFinding>,
}

/// Being inside a git work tree is not enough: a path can sit in a repo
/// and have no commits of its own, which draws an empty scatter and calls
/// it a view.
pub fn has_history(dir: &Path, root: &str) -> bool {
    !churn(dir, root).is_empty()
}

/// `git log --numstat`, parsed. Read-only, and a missing or empty history
/// yields an empty map rather than an error — a crate does not have to be
/// in git to be analysed.
fn churn(dir: &Path, root: &str) -> BTreeMap<String, Churn> {
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
        .output();
    let Ok(out) = out else { return BTreeMap::new() };
    if !out.status.success() {
        return BTreeMap::new();
    }
    let text = String::from_utf8_lossy(&out.stdout);

    let mut map: BTreeMap<String, Churn> = BTreeMap::new();
    let mut authors: BTreeMap<String, std::collections::BTreeSet<String>> = BTreeMap::new();
    let (mut author, mut date) = (String::new(), String::new());

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix('\u{1}') {
            let mut p = rest.split('\u{1}');
            let _hash = p.next();
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
        let e = map.entry(rel.clone()).or_default();
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
        if let Some(e) = map.get_mut(&f) {
            e.author_count = set.len();
        }
    }
    map
}

pub fn build(s: &Scorecard) -> Result<Hotspots> {
    let unclassified = s.config.layers.len();
    let ch = churn(&s.dir, &s.config.root);

    let file_nodes: Vec<FileNode> = s
        .model
        .modules
        .iter()
        .map(|m| {
            let syms: Vec<_> = s
                .model
                .symbols
                .iter()
                .filter(|x| x.module == m.id && x.parent.is_none())
                .collect();
            let t = syms
                .iter()
                .filter_map(|x| s.classified.layer_of(&x.id))
                .filter_map(|l| tier_index(s, l))
                .min()
                .unwrap_or(unclassified);
            FileNode {
                f: m.file.clone(),
                m: m.id.clone(),
                p: syms
                    .iter()
                    .filter_map(|x| s.classified.pattern_of(&x.id))
                    .next()
                    .unwrap_or("unclassified")
                    .to_string(),
                t,
                cls: m.id.rsplit("::").next().unwrap_or(&m.id).to_string(),
                loc: m.loc,
            }
        })
        .collect();

    let by_module: BTreeMap<&str, &str> = s
        .model
        .modules
        .iter()
        .map(|m| (m.id.as_str(), m.file.as_str()))
        .collect();
    let mut file_links: Vec<(String, String)> = s
        .model
        .file_links()
        .into_iter()
        .filter_map(|(a, b)| {
            Some((
                by_module.get(a.as_str())?.to_string(),
                by_module.get(b.as_str())?.to_string(),
            ))
        })
        .collect();
    file_links.sort();
    file_links.dedup();

    let commits: usize = ch.values().map(|c| c.commits).sum();
    let since = ch
        .values()
        .map(|c| c.first.clone())
        .min()
        .unwrap_or_default();

    Ok(Hotspots {
        generated_at: crate::baseline::now_iso(),
        project: s.model.project.clone(),
        root: s.config.root.clone(),
        stats: stats(s),
        coverage: s.coverage(),
        tiers: tiers(s),
        file_nodes,
        file_links,
        churn_meta: ChurnMeta {
            since,
            commits,
            files: ch.len(),
            available: !ch.is_empty(),
        },
        churn: ch,
        findings: vec![],
    })
}
