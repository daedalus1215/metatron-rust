//! `hotspots` — git churn against dependents. Entirely language-agnostic,
//! and the only view that produces a real result on a crate with no
//! architecture yet: a file that changes constantly and that everything
//! depends on is a problem in any language.

use super::{stats, tier_index, tiers, Stats, Tier};
use crate::churn::{Churn, FileChurn};
use crate::scorecard::Scorecard;
use anyhow::Result;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Serialize)]
pub struct ChurnMeta {
    pub since: String,
    pub commits: usize,
    pub files: usize,
    pub available: bool,
    /// Why churn is unavailable. Spec 08: an empty scatter under a caption
    /// about churn is a picture of nothing, and the reader cannot tell an
    /// untracked crate from a broken tool.
    pub reason: String,
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
    pub churn: BTreeMap<String, FileChurn>,
    #[serde(rename = "churnMeta")]
    pub churn_meta: ChurnMeta,
}

/// Being inside a git work tree is not enough: a path can sit in a repo
/// and have no commits of its own, which draws an empty scatter and calls
/// it a view.
pub fn has_history(dir: &Path, root: &str) -> bool {
    Churn::has_commits(dir, root)
}

pub fn build(s: &Scorecard) -> Result<Hotspots> {
    let unclassified = s.config.layers.len();
    let ch = Churn::measure(&s.dir, &s.config.root, &s.model);

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
            since: ch.window.since.clone(),
            commits: ch.window.commits,
            files: ch.window.files,
            available: ch.available,
            reason: ch.reason.clone(),
        },
        churn: ch.files,
    })
}
