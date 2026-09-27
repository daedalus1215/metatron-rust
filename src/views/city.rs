//! `city` — the free win. The template already renders one tower per
//! module and one floor per directory, with the isometric projection that
//! makes tower heights comparable by eye. Only the payload changes:
//!
//! ```text
//! tower = module      floor = type      floor colour = layer
//! ```
//!
//! A tower whose floors are different colours **is** a mixed-layer module,
//! visible at a glance and needing no legend. No template change.

use super::{stats, tier_index, tiers, Stats, Tier};
use crate::model::SymbolKind;
use crate::rules::Status;
use crate::scorecard::Scorecard;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub struct Floor {
    pub id: String,
    pub label: String,
    /// Layer index. metatron's `tier` slot, filled by the ordered layers
    /// from `metatron.toml`.
    pub tier: usize,
    /// Floor height: methods, so a god object is a tall floor.
    pub count: usize,
    pub patterns: Vec<String>,
    pub files: Vec<FileRef>,
}

#[derive(Serialize)]
pub struct FileRef {
    pub f: String,
    pub p: String,
}

#[derive(Serialize)]
pub struct Module {
    pub id: String,
    pub files: usize,
    pub floors: Vec<Floor>,
    #[serde(rename = "tiersPresent")]
    pub tiers_present: Vec<usize>,
    pub absent: Vec<usize>,
    pub findings: Vec<Finding>,
    /// metatron's endpoint count per tower. Here: entry points.
    pub endpoints: usize,
}

#[derive(Serialize)]
pub struct Finding {
    pub id: String,
    pub title: String,
}

#[derive(Serialize)]
pub struct City {
    #[serde(rename = "generatedAt")]
    pub generated_at: String,
    pub stats: Stats,
    pub tiers: Vec<Tier>,
    pub modules: Vec<Module>,
    #[serde(rename = "domainEdges")]
    pub domain_edges: Vec<(String, String)>,
    pub findings: Vec<CityFinding>,
}

#[derive(Serialize)]
pub struct CityFinding {
    pub id: String,
    pub title: String,
    pub tone: String,
    /// Read by atlas's ledger cards.
    pub detail: String,
    pub items: Vec<String>,
}

pub fn build(s: &Scorecard) -> City {
    let unclassified = s.config.layers.len();

    let mut by_module: BTreeMap<&str, Vec<Floor>> = BTreeMap::new();

    // Every module gets a tower, including one holding only functions.
    // `arioch::ui` is 1,300 lines of render fns and no types; dropping it
    // would hide the module where `renders-off-store` fires.
    for m in &s.model.modules {
        // One functions floor **per layer**, not one per module. Taking
        // the minimum tier would hide `load_config` behind `handle_key`
        // and make a mixed-layer module read as a single colour — which
        // is the one thing this view is supposed to make obvious.
        let mut by_tier: BTreeMap<usize, Vec<&crate::model::Symbol>> = BTreeMap::new();
        for x in s
            .model
            .symbols
            .iter()
            .filter(|x| x.module == m.id && x.parent.is_none() && !x.is_test)
            .filter(|x| x.kind == crate::model::SymbolKind::Fn)
        {
            let tier = s
                .classified
                .layer_of(&x.id)
                .and_then(|l| tier_index(s, l))
                .unwrap_or(unclassified);
            by_tier.entry(tier).or_default().push(x);
        }
        let split = by_tier.len() > 1;
        for (tier, fns) in by_tier {
            let mut patterns: Vec<String> = fns
                .iter()
                .filter_map(|x| s.classified.pattern_of(&x.id))
                .map(str::to_string)
                .collect();
            patterns.sort();
            patterns.dedup();
            let label = if split && !patterns.is_empty() {
                format!("({})", patterns.join(", "))
            } else {
                "(functions)".to_string()
            };
            by_module.entry(m.id.as_str()).or_default().push(Floor {
                id: format!("{}::(functions{tier})", m.id),
                label,
                tier,
                count: fns.len(),
                patterns,
                files: vec![FileRef { f: m.file.clone(), p: "functions".into() }],
            });
        }
    }

    for sym in &s.model.symbols {
        // A floor is a type. Free functions live in the module, not on a
        // floor of their own — otherwise a 60-function module is a
        // 60-storey tower and height stops meaning anything.
        if !matches!(sym.kind, SymbolKind::Struct | SymbolKind::Enum | SymbolKind::Trait | SymbolKind::Union)
            || sym.is_test
        {
            continue;
        }
        let layer = s.classified.layer_of(&sym.id);
        let tier = layer.and_then(|l| tier_index(s, l)).unwrap_or(unclassified);
        let methods = s
            .model
            .symbols
            .iter()
            .filter(|m| m.parent.as_deref() == Some(sym.id.as_str()))
            .count();
        by_module.entry(sym.module.as_str()).or_default().push(Floor {
            id: sym.id.clone(),
            label: sym.name.clone(),
            tier,
            count: methods.max(1),
            patterns: s
                .classified
                .pattern_of(&sym.id)
                .map(|p| vec![p.to_string()])
                .unwrap_or_default(),
            files: vec![FileRef {
                f: sym.file.clone(),
                p: s.classified.pattern_of(&sym.id).unwrap_or("unclassified").to_string(),
            }],
        });
    }

    // Findings hang off the tower they occur in, so a module carries its
    // own deviations.
    let mut per_module: BTreeMap<&str, Vec<Finding>> = BTreeMap::new();
    let mut findings = Vec::new();
    for f in &s.report.findings {
        if f.status != Status::Violated {
            continue;
        }
        findings.push(CityFinding {
            id: f.id.into(),
            title: f.title.into(),
            tone: format!("{:?}", f.tone).to_lowercase(),
            detail: detail_of(f),
            items: f.instances.iter().map(|i| format!("{}:{} {}", i.file, i.line, i.detail)).collect(),
        });
        for i in &f.instances {
            let Some(sym) = s.model.symbol(&i.from) else { continue };
            let list = per_module.entry(sym.module.as_str()).or_default();
            if !list.iter().any(|x| x.id == f.id) {
                list.push(Finding { id: f.id.into(), title: f.title.into() });
            }
        }
    }

    let all: Vec<usize> = (0..s.config.layers.len()).collect();
    let mut modules: Vec<Module> = by_module
        .into_iter()
        .map(|(m, mut floors)| {
            floors.sort_by(|a, b| b.tier.cmp(&a.tier).then(a.label.cmp(&b.label)));
            let mut present: Vec<usize> = floors.iter().map(|f| f.tier).collect();
            present.sort_unstable();
            present.dedup();
            Module {
                id: super::mod_label(m, &s.model.project),
                files: floors.iter().map(|f| f.count).sum(),
                absent: all.iter().copied().filter(|i| !present.contains(i)).collect(),
                tiers_present: present,
                floors,
                findings: per_module.remove(m).unwrap_or_default(),
                endpoints: super::traffic::entries(s).iter().filter(|e| e.module == m).count(),
            }
        })
        .collect();
    modules.sort_by(|a, b| b.files.cmp(&a.files).then(a.id.cmp(&b.id)));

    City {
        generated_at: crate::baseline::now_iso(),
        stats: stats(s),
        tiers: tiers(s),
        modules,
        domain_edges: s.model.file_links(),
        findings,
    }
}

/// Ledger prose for one rule, from its own metadata. Never typed into a
/// template — that is the mistake `narrate` exists to prevent.
pub fn detail_of(f: &crate::rules::Finding) -> String {
    let kind = if f.kind == crate::rules::Kind::Heuristic {
        "A heuristic: reported, never gated."
    } else {
        "Decidable from the model."
    };
    format!(
        "{} · enforced by {:?}, source {}. {kind}",
        f.title, f.tier, f.source
    )
}
