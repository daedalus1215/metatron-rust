//! `layers` — symbols on the plane of their layer, and the one template
//! change that matters.
//!
//! `impl ActivityStore for SqliteActivityStore` runs from infrastructure
//! back up to domain. Drawn like an ordinary dependency it looks like the
//! worst violation on the diagram; it is in fact the dependency inversion
//! the architecture is built on. The payload marks those links `k: "impl"`
//! so the template can stroke them apart.
//!
//! This is also the answer to why file-level tools cannot draw this
//! architecture at all: that arrow corresponds to no import.

use super::{stats, tier_index, tiers, Stats, Tier};
use crate::model::{EdgeKind, EdgeTarget, SymbolKind};
use crate::rules::Status;
use crate::scorecard::Scorecard;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Serialize)]
pub struct Node {
    pub i: usize,
    pub f: String,
    pub m: String,
    pub p: String,
    pub t: usize,
    pub cls: String,
    pub loc: u32,
    pub x: f64,
    pub z: f64,
    /// Severity: `ok` or `warn`.
    pub s: &'static str,
    /// Which rules flagged this symbol.
    pub w: Vec<String>,
}

/// Serialised as the array the template indexes:
/// `[from, to, crosses-a-layer, rule-id, kind]`. An object here renders a
/// blank canvas with no console error, because `l[0]` on an object is
/// simply `undefined` — which is exactly the failure spec 06 says static
/// checks will not catch.
#[derive(Serialize, Clone)]
#[serde(into = "(usize, usize, u8, Option<String>, &'static str)")]
pub struct Link {
    pub a: usize,
    pub b: usize,
    /// Runs against the layer ordering.
    pub up: bool,
    /// The rule this link violates, if any.
    pub rule: Option<String>,
    /// Edge kind. `impl` is drawn apart from everything else.
    pub k: &'static str,
}

impl From<Link> for (usize, usize, u8, Option<String>, &'static str) {
    fn from(l: Link) -> Self {
        (l.a, l.b, l.up as u8, l.rule, l.k)
    }
}

#[derive(Serialize)]
pub struct Plane {
    pub t: usize,
    pub name: String,
    pub sub: String,
    pub count: usize,
    pub extra: bool,
    pub modules: Vec<PlaneModule>,
    pub patterns: Vec<String>,
}

#[derive(Serialize)]
pub struct PlaneModule {
    pub m: String,
    pub n: usize,
}

#[derive(Serialize)]
pub struct Layers {
    #[serde(rename = "generatedAt")]
    pub generated_at: String,
    pub stats: Stats,
    pub tiers: Vec<Tier>,
    pub extent: f64,
    pub planes: Vec<Plane>,
    pub modules: Vec<ModulePos>,
    pub nodes: Vec<Node>,
    pub links: Vec<Link>,
    pub findings: Vec<super::city::CityFinding>,
    /// `{id, sev, why}` — the shape `layers.html` indexes by rule id to
    /// decide how to stroke a flagged link and how to label the legend.
    #[serde(rename = "skipRules")]
    pub skip_rules: Vec<SkipRule>,
    /// Counted so the legend can say how many inversion arrows there are
    /// without anyone having to find them.
    pub inversions: usize,
}

#[derive(Serialize)]
pub struct SkipRule {
    pub id: String,
    /// `crit` for a rule that fails the build, `warn` otherwise.
    pub sev: &'static str,
    pub why: String,
}

#[derive(Serialize)]
pub struct ModulePos {
    pub m: String,
    pub n: usize,
    pub x: f64,
    pub z: f64,
}

pub fn build(s: &Scorecard) -> Layers {
    let unclassified = s.config.layers.len();

    let drawn: Vec<&crate::model::Symbol> = s
        .model
        .symbols
        .iter()
        .filter(|x| !x.is_test && x.parent.is_none())
        .filter(|x| !matches!(x.kind, SymbolKind::Module))
        .collect();

    // Modules get a ring position; symbols sit near their module on the
    // plane of their layer. Deterministic, so a redraw is comparable.
    let mut mod_count: BTreeMap<&str, usize> = BTreeMap::new();
    for x in &drawn {
        *mod_count.entry(x.module.as_str()).or_default() += 1;
    }
    let mods: Vec<&str> = mod_count.keys().copied().collect();
    let n = mods.len().max(1) as f64;
    let radius = 16.0 + n * 1.6;
    let centres: BTreeMap<&str, (f64, f64)> = mods
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let a = (i as f64 / n) * std::f64::consts::TAU;
            (*m, (radius * a.cos(), radius * a.sin()))
        })
        .collect();

    let mut flagged: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for f in &s.report.findings {
        if f.status != Status::Violated {
            continue;
        }
        for i in &f.instances {
            flagged.entry(i.from.as_str()).or_default().push(f.id.to_string());
        }
    }

    let mut index: BTreeMap<&str, usize> = BTreeMap::new();
    let mut nodes = Vec::new();
    let mut per_mod: BTreeMap<&str, usize> = BTreeMap::new();
    for x in &drawn {
        let layer = s.classified.layer_of(&x.id);
        let t = layer.and_then(|l| tier_index(s, l)).unwrap_or(unclassified);
        let (cx, cz) = centres[x.module.as_str()];
        let k = per_mod.entry(x.module.as_str()).or_default();
        // A small deterministic spiral inside the module's cell.
        let a = *k as f64 * 2.399_963;
        let r = 1.6 * (*k as f64).sqrt();
        *k += 1;
        let i = nodes.len();
        index.insert(x.id.as_str(), i);
        let w = flagged.get(x.id.as_str()).cloned().unwrap_or_default();
        nodes.push(Node {
            i,
            f: x.file.clone(),
            m: super::mod_label(&x.module, &s.model.project),
            p: s.classified.pattern_of(&x.id).unwrap_or("unclassified").to_string(),
            t,
            cls: x.name.clone(),
            loc: x.loc,
            x: ((cx + r * a.cos()) * 100.0).round() / 100.0,
            z: ((cz + r * a.sin()) * 100.0).round() / 100.0,
            s: if w.is_empty() { "ok" } else { "warn" },
            w,
        });
    }

    let mut links = Vec::new();
    let mut inversions = 0;
    let mut seen = std::collections::BTreeSet::new();
    for e in &s.model.edges {
        let EdgeTarget::Local { id: to } = &e.to else { continue };
        // Methods are drawn as their type.
        let from = s
            .model
            .symbol(&e.from)
            .and_then(|x| x.parent.clone())
            .unwrap_or_else(|| e.from.clone());
        let to_owner = s
            .model
            .symbol(to)
            .and_then(|x| x.parent.clone())
            .unwrap_or_else(|| to.clone());
        let (Some(&a), Some(&b)) = (index.get(from.as_str()), index.get(to_owner.as_str()))
        else {
            continue;
        };
        if a == b {
            continue;
        }
        let k = match e.kind {
            EdgeKind::Impl => "impl",
            EdgeKind::Bound => "bound",
            EdgeKind::Use => "use",
            EdgeKind::Field => "field",
            EdgeKind::Sig => "sig",
            EdgeKind::Call => "call",
        };
        if !seen.insert((a, b, k)) {
            continue;
        }
        let up = nodes[b].t < nodes[a].t;
        if e.kind == EdgeKind::Impl {
            inversions += 1;
        }
        // The inversion arrow also runs upward, and must never be
        // reported as a skip: it is the architecture, not a violation.
        let rule = if e.kind == EdgeKind::Impl {
            None
        } else {
            flagged
                .get(from.as_str())
                .and_then(|rs| rs.first())
                .cloned()
        };
        links.push(Link { a, b, up, rule, k });
    }

    let mut planes: Vec<Plane> = Vec::new();
    for (i, l) in s.config.layers.iter().enumerate() {
        let on: Vec<&Node> = nodes.iter().filter(|x| x.t == i).collect();
        let mut by: BTreeMap<&str, usize> = BTreeMap::new();
        for x in &on {
            *by.entry(x.m.as_str()).or_default() += 1;
        }
        let mut modules: Vec<PlaneModule> =
            by.into_iter().map(|(m, n)| PlaneModule { m: m.into(), n }).collect();
        modules.sort_by(|a, b| b.n.cmp(&a.n).then(a.m.cmp(&b.m)));
        let mut patterns: Vec<String> = on.iter().map(|x| x.p.clone()).collect();
        patterns.sort();
        patterns.dedup();
        planes.push(Plane {
            t: i,
            name: l.title.clone(),
            sub: l.sub.clone(),
            count: on.len(),
            extra: false,
            modules,
            patterns,
        });
    }
    // The plane for everything the config does not recognise. Drawn,
    // because a symbol the classifier missed is the one most worth seeing.
    let un: Vec<&Node> = nodes.iter().filter(|x| x.t == unclassified).collect();
    if !un.is_empty() {
        let mut by: BTreeMap<&str, usize> = BTreeMap::new();
        for x in &un {
            *by.entry(x.m.as_str()).or_default() += 1;
        }
        let mut modules: Vec<PlaneModule> =
            by.into_iter().map(|(m, n)| PlaneModule { m: m.into(), n }).collect();
        modules.sort_by(|a, b| b.n.cmp(&a.n).then(a.m.cmp(&b.m)));
        planes.push(Plane {
            t: unclassified,
            name: "Unclassified".into(),
            sub: "matched no pattern in metatron.toml".into(),
            count: un.len(),
            extra: true,
            modules,
            patterns: vec![],
        });
    }

    let extent = nodes
        .iter()
        .map(|n| n.x.abs().max(n.z.abs()))
        .fold(0.0f64, f64::max)
        + 12.0;

    Layers {
        generated_at: crate::baseline::now_iso(),
        stats: stats(s),
        tiers: tiers(s),
        extent,
        planes,
        modules: mods
            .iter()
            .map(|m| ModulePos {
                m: super::mod_label(m, &s.model.project),
                n: mod_count[m],
                x: (centres[m].0 * 100.0).round() / 100.0,
                z: (centres[m].1 * 100.0).round() / 100.0,
            })
            .collect(),
        nodes,
        links,
        findings: super::city::build(s).findings,
        skip_rules: s
            .report
            .findings
            .iter()
            .filter(|f| f.status == Status::Violated)
            .map(|f| SkipRule {
                id: f.id.to_string(),
                sev: if f.gate { "crit" } else { "warn" },
                why: format!("{} — ", f.title),
            })
            .collect(),
        inversions,
    }
}
