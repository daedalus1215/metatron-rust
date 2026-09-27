//! `atlas` — the module map plus the conformance ledger. Works as-is on
//! the module-level projection from spec 01.

use super::{stats, tier_index, tiers, Stats, Tier};
use crate::rules::Status;
use crate::scorecard::Scorecard;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub module: String,
    pub tier: usize,
    pub count: usize,
    pub loc: u32,
    pub patterns: Vec<String>,
}

/// Field names follow what `atlas.html` reads, not what reads best here:
/// `path` is `/`-separated and basenamed by the template, and a consumer
/// is an object with `module`, `pattern` and `f`.
#[derive(Serialize)]
pub struct Port {
    pub path: String,
    pub name: String,
    pub owner: String,
    pub consumers: Vec<Consumer>,
    pub impls: usize,
    pub fakes: usize,
}

#[derive(Serialize)]
pub struct Consumer {
    pub module: String,
    pub pattern: String,
    pub f: String,
}

#[derive(Serialize)]
pub struct Atlas {
    #[serde(rename = "generatedAt")]
    pub generated_at: String,
    pub stats: Stats,
    pub tiers: Vec<Tier>,
    pub nodes: Vec<Node>,
    pub edges: Vec<(String, String)>,
    pub modules: Vec<String>,
    #[serde(rename = "domainModules")]
    pub domain_modules: Vec<String>,
    #[serde(rename = "platformModules")]
    pub platform_modules: Vec<String>,
    pub ports: Vec<Port>,
    #[serde(rename = "crossDomain")]
    pub cross_domain: Vec<(String, String)>,
    pub shape: BTreeMap<String, usize>,
    pub findings: Vec<super::city::CityFinding>,
}

pub fn build(s: &Scorecard) -> Atlas {
    let unclassified = s.config.layers.len();
    let nodes: Vec<Node> = s
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
            let mut patterns: Vec<String> = syms
                .iter()
                .filter_map(|x| s.classified.pattern_of(&x.id))
                .map(str::to_string)
                .collect();
            patterns.sort();
            patterns.dedup();
            // A module's tier is the innermost layer it holds; a module
            // spanning several is what the mixed-layer rule is for, and
            // the patterns list shows it.
            let tier = syms
                .iter()
                .filter_map(|x| s.classified.layer_of(&x.id))
                .filter_map(|l| tier_index(s, l))
                .min()
                .unwrap_or(unclassified);
            Node {
                id: m.id.clone(),
                label: super::mod_label(
                    m.id.rsplit("::").next().unwrap_or(&m.id),
                    &s.model.project,
                ),
                module: m.id.clone(),
                tier,
                count: syms.len(),
                loc: m.loc,
                patterns,
            }
        })
        .collect();

    let ports: Vec<Port> = s
        .model
        .symbols
        .iter()
        .filter(|x| s.classified.pattern_of(&x.id) == Some("port"))
        .filter(|x| x.kind == crate::model::SymbolKind::Trait)
        .map(|p| {
            let impls: Vec<_> = s
                .model
                .impls
                .iter()
                .filter(|b| b.trait_id.as_deref() == Some(p.id.as_str()))
                .collect();
            let fakes = impls
                .iter()
                .filter(|b| b.is_test || b.file.ends_with("mem.rs"))
                .count();
            let mut consumers: Vec<Consumer> = s
                .model
                .edges
                .iter()
                .filter(|e| e.kind == crate::model::EdgeKind::Bound)
                .filter(|e| matches!(&e.to, crate::model::EdgeTarget::Local { id } if id == &p.id))
                .filter_map(|e| s.model.symbol(&e.from))
                .map(|x| Consumer {
                    module: x.module.clone(),
                    pattern: s
                        .classified
                        .pattern_of(&x.id)
                        .unwrap_or("unclassified")
                        .to_string(),
                    f: format!("{} · {}", x.name, x.file),
                })
                .collect();
            consumers.sort_by(|a, b| a.f.cmp(&b.f));
            consumers.dedup_by(|a, b| a.f == b.f);
            Port {
                path: p.file.replace('\\', "/"),
                name: p.name.clone(),
                owner: p.module.clone(),
                consumers,
                impls: impls.len(),
                fakes,
            }
        })
        .collect();

    let mut shape: BTreeMap<String, usize> = BTreeMap::new();
    for c in s.classified.by_symbol.values().filter(|c| !c.inherited) {
        *shape.entry(c.pattern.clone()).or_default() += 1;
    }
    if !s.classified.unmatched.is_empty() {
        shape.insert("unclassified".into(), s.classified.unmatched.len());
    }

    // The template's "platform" toggle hides infrastructure. Everything
    // else — including modules the classifier did not recognise — belongs
    // on the default map: against a crate at 1% coverage, filtering to
    // classified modules would draw an empty page and call it an atlas.
    let mut platform_modules: Vec<String> = s
        .model
        .symbols
        .iter()
        .filter(|x| s.classified.layer_of(&x.id) == Some("infrastructure"))
        .map(|x| x.module.clone())
        .collect();
    platform_modules.sort();
    platform_modules.dedup();
    let domain_modules: Vec<String> = s
        .model
        .modules
        .iter()
        .map(|m| m.id.clone())
        .filter(|m| !platform_modules.contains(m))
        .collect();

    Atlas {
        generated_at: crate::baseline::now_iso(),
        stats: stats(s),
        tiers: tiers(s),
        nodes,
        edges: s.model.file_links(),
        modules: s.model.modules.iter().map(|m| m.id.clone()).collect(),
        domain_modules,
        platform_modules,
        ports,
        cross_domain: vec![],
        shape,
        findings: s
            .report
            .findings
            .iter()
            .filter(|f| f.status == Status::Violated)
            .map(|f| super::city::CityFinding {
                id: f.id.into(),
                title: f.title.into(),
                tone: format!("{:?}", f.tone).to_lowercase(),
                detail: super::city::detail_of(f),
                items: f
                    .instances
                    .iter()
                    .map(|i| format!("{}:{} {}", i.file, i.line, i.detail))
                    .collect(),
            })
            .collect(),
    }
}
