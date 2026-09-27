//! Turning `patterns-rust` into a config the scanner can execute, and
//! reporting honestly how much of the code it recognised.
//! See `specs/03-classifier-and-config.md`.
//!
//! metatron-nestjs classifies by filename — `.repository.ts` is a
//! repository. Rust has no such convention and should not grow one. The
//! glossary carries its conventions as a **module path** plus a **symbol
//! shape**, and every pattern page states both. Two signals is more than a
//! filename can give, not less: a path and a shape can say different
//! things about two symbols in the same file, which is what makes spec
//! 02's mixed-layer detector possible at all.

use crate::model::{EdgeTarget, Model, Symbol, SymbolKind};
use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const PATTERNS_RUST: &str = include_str!("profiles/patterns-rust.toml");

// ------------------------------------------------------------------ config

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SymbolMatch {
    /// One kind or several. `kind = "trait"` and `kind = ["struct","enum"]`
    /// both parse.
    #[serde(default)]
    pub kind: Kinds,
    pub name: Option<String>,
    pub has_field: Option<String>,
    pub not_has_field: Option<String>,
    #[serde(default)]
    pub derives: Vec<String>,
    pub returns: Option<String>,
    /// The symbol is a type that `impl`s a locally-defined trait which is
    /// itself classified as a port. This is the seam, and it is the one
    /// predicate that needs a second pass.
    pub implements_port: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Kinds {
    #[default]
    Any,
    One(String),
    Many(Vec<String>),
}

impl Kinds {
    fn matches(&self, k: SymbolKind) -> bool {
        let name = match k {
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Trait => "trait",
            SymbolKind::Union => "union",
            SymbolKind::TypeAlias => "type",
            SymbolKind::Fn => "fn",
            SymbolKind::Method => "method",
            SymbolKind::Static => "static",
            SymbolKind::Const => "const",
            SymbolKind::Module => "mod",
        };
        match self {
            Kinds::Any => true,
            Kinds::One(s) => s == name,
            Kinds::Many(v) => v.iter().any(|s| s == name),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Pattern {
    pub id: String,
    pub layer: String,
    /// Prefix match on the file path, or a regex when it starts with `^`.
    pub path: Option<String>,
    pub symbol: Option<SymbolMatch>,
    /// Crates or crate paths the symbol must reach for this pattern to
    /// apply. Present only where the glossary distinguishes two patterns
    /// by what they talk to.
    #[serde(default)]
    pub externs: Vec<String>,
    /// Patterns that a scanner cannot reliably tell apart share a group.
    pub group: Option<String>,
    #[serde(default)]
    pub pure: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Layer {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub sub: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Naming {
    #[serde(default)]
    pub forbid_in_domain: Vec<String>,
    pub use_case_params: Option<String>,
    pub service_input: Option<String>,
    pub domain_output: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default = "src_default")]
    pub root: String,
    pub name: Option<String>,
    /// `"patterns-rust"`, or absent for a config that stands alone.
    pub extends: Option<String>,
    #[serde(default)]
    pub flow: Vec<String>,
    #[serde(default, rename = "layer")]
    pub layers: Vec<Layer>,
    #[serde(default, rename = "pattern")]
    pub patterns: Vec<Pattern>,
    /// Prepended to the inherited list, the way metatron's `addPatterns`
    /// works. First match wins, so a project rule beats a profile rule.
    #[serde(default, rename = "add_pattern")]
    pub add_patterns: Vec<Pattern>,
    #[serde(default)]
    pub externs: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub naming: Naming,
    /// `crate-graph.md`: "A" is a single crate (convention + a CI test),
    /// "B" is a workspace split where the compiler rejects an illegal
    /// import. Under B the three layer rules move to `tier: compiler` and
    /// stop being checked — a rule the build already guarantees does not
    /// need a second opinion. Not auto-detected: being a workspace is not
    /// the same as having `domain` as a crate that cannot see `rusqlite`.
    #[serde(default = "crate_graph_default")]
    pub crate_graph: String,
}

fn crate_graph_default() -> String {
    "A".into()
}

fn src_default() -> String {
    "src".into()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            root: src_default(),
            name: None,
            extends: Some("patterns-rust".into()),
            flow: vec![],
            layers: vec![],
            patterns: vec![],
            add_patterns: vec![],
            externs: BTreeMap::new(),
            naming: Naming::default(),
            crate_graph: crate_graph_default(),
        }
    }
}

impl Config {
    pub fn profile() -> Self {
        toml::from_str(PATTERNS_RUST).expect("built-in profile is malformed")
    }

    /// Read `metatron.toml` beside `Cargo.toml`. Absent, the project gets
    /// the glossary unmodified — which is the right default for a tool
    /// whose job is to measure distance from that glossary.
    pub fn load(dir: &Path) -> Result<Self> {
        let p = dir.join("metatron.toml");
        if !p.exists() {
            return Ok(Self::profile());
        }
        let text = std::fs::read_to_string(&p)
            .with_context(|| format!("reading {}", p.display()))?;
        let mut cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing {}", p.display()))?;
        cfg.resolve();
        Ok(cfg)
    }

    fn resolve(&mut self) {
        if self.extends.as_deref() != Some("patterns-rust") {
            return;
        }
        let base = Self::profile();
        if self.patterns.is_empty() {
            self.patterns = base.patterns;
        }
        // `add_pattern` prepends: a project rule wins over a profile rule.
        let mut add = std::mem::take(&mut self.add_patterns);
        add.append(&mut self.patterns);
        self.patterns = add;

        if self.layers.is_empty() {
            self.layers = base.layers;
        }
        if self.flow.is_empty() {
            self.flow = base.flow;
        }
        if self.externs.is_empty() {
            self.externs = base.externs;
        }
        if self.naming.forbid_in_domain.is_empty() {
            self.naming = base.naming;
        }
    }
}

// ------------------------------------------------------------- the result

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Classification {
    pub symbol: String,
    pub pattern: String,
    pub layer: String,
    /// The pattern was decided by the symbol's own shape, or inherited
    /// from the type it belongs to.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub inherited: bool,
    /// Several patterns in one group matched, or none did. The label is
    /// the group; the distinction was not guessed.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ambiguous: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Unmatched {
    pub symbol: String,
    pub kind: SymbolKind,
    pub file: String,
    pub line: u32,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Coverage {
    pub classified: usize,
    pub total: usize,
    pub ports: usize,
    pub by_pattern: BTreeMap<String, usize>,
    pub by_layer: BTreeMap<String, usize>,
    /// Unmatched symbols grouped as `<kind> in <dir>`, with an example.
    pub gaps: Vec<(String, usize, String)>,
}

impl Coverage {
    pub fn ratio(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            self.classified as f64 / self.total as f64
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Classified {
    pub by_symbol: BTreeMap<String, Classification>,
    pub unmatched: Vec<Unmatched>,
    pub coverage: Coverage,
    pub ambiguities: Vec<String>,
}

impl Classified {
    pub fn layer_of(&self, id: &str) -> Option<&str> {
        self.by_symbol.get(id).map(|c| c.layer.as_str())
    }
    pub fn pattern_of(&self, id: &str) -> Option<&str> {
        self.by_symbol.get(id).map(|c| c.pattern.as_str())
    }
    /// Modules holding symbols from more than one layer. Spec 02's
    /// detector 2, which needed this spec to exist.
    pub fn mixed_layer_modules(&self, model: &Model) -> Vec<(String, BTreeSet<String>)> {
        let mut by: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
        for s in &model.symbols {
            if s.is_test || matches!(s.kind, SymbolKind::Module) {
                continue;
            }
            // A method belongs to its type; counting it separately would
            // report every module holding one impl block as mixed.
            if s.parent.is_some() {
                continue;
            }
            if let Some(c) = self.by_symbol.get(&s.id) {
                by.entry(s.module.as_str()).or_default().insert(c.layer.clone());
            }
        }
        by.into_iter()
            .filter(|(_, l)| l.len() > 1)
            .map(|(m, l)| (m.to_string(), l))
            .collect()
    }
}

// ------------------------------------------------------------- classifying

/// Everything reachable from a symbol as an extern path, including through
/// its methods and fields — `SqliteActivityStore` names `rusqlite` on a
/// field, `RealFs` names `std::fs` in a method body.
fn extern_reach<'a>(model: &'a Model, id: &str) -> BTreeSet<&'a str> {
    let owned: BTreeSet<&str> = model
        .symbols
        .iter()
        .filter(|s| s.id == id || s.parent.as_deref() == Some(id))
        .map(|s| s.id.as_str())
        .collect();
    model
        .edges
        .iter()
        .filter(|e| owned.contains(e.from.as_str()))
        .filter_map(|e| match &e.to {
            EdgeTarget::Extern { path, .. } => Some(path.as_str()),
            _ => None,
        })
        .collect()
}

fn path_matches(pat: &str, file: &str, cache: &mut BTreeMap<String, Option<Regex>>) -> bool {
    if !pat.starts_with('^') {
        return file.starts_with(pat);
    }
    let re = cache
        .entry(pat.to_string())
        .or_insert_with(|| Regex::new(pat).ok());
    re.as_ref().is_some_and(|r| r.is_match(file))
}

fn shape_matches(
    m: &SymbolMatch,
    s: &Symbol,
    ports: &BTreeSet<String>,
    impls_of: &BTreeMap<&str, BTreeSet<&str>>,
    cache: &mut BTreeMap<String, Option<Regex>>,
) -> bool {
    if !m.kind.matches(s.kind) {
        return false;
    }
    if let Some(n) = &m.name {
        let re = cache.entry(n.clone()).or_insert_with(|| Regex::new(n).ok());
        if !re.as_ref().is_some_and(|r| r.is_match(&s.name)) {
            return false;
        }
    }
    if let Some(f) = &m.has_field {
        if !s.fields.iter().any(|x| &x.name == f) {
            return false;
        }
    }
    if let Some(f) = &m.not_has_field {
        if s.fields.iter().any(|x| &x.name == f) {
            return false;
        }
    }
    if !m.derives.iter().all(|d| s.derives.contains(d)) {
        return false;
    }
    if let Some(r) = &m.returns {
        let ret = s.sig.as_ref().and_then(|g| g.ret.as_deref()).unwrap_or("");
        if !ret.contains(r) {
            return false;
        }
    }
    if m.implements_port == Some(true) {
        let hit = impls_of
            .get(s.id.as_str())
            .is_some_and(|ts| ts.iter().any(|t| ports.contains(*t)));
        if !hit {
            return false;
        }
    }
    true
}

pub fn classify(model: &Model, cfg: &Config) -> Classified {
    let mut cache: BTreeMap<String, Option<Regex>> = BTreeMap::new();

    // Which local trait does each type implement? Needed for
    // `implements_port`, and the reason classification is two passes.
    let mut impls_of: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for b in &model.impls {
        if let (Some(t), Some(tr)) = (&b.type_id, &b.trait_id) {
            impls_of.entry(t.as_str()).or_default().insert(tr.as_str());
        }
    }

    // Pass 1: ports only. A port is decided by path and kind alone, so it
    // needs nothing from this pass.
    let port_patterns: Vec<&Pattern> = cfg.patterns.iter().filter(|p| p.id == "port").collect();
    let mut ports: BTreeSet<String> = BTreeSet::new();
    for s in &model.symbols {
        for p in &port_patterns {
            let ok_path = p
                .path
                .as_ref()
                .is_none_or(|x| path_matches(x, &s.file, &mut cache));
            let ok_shape = p.symbol.as_ref().is_none_or(|m| {
                shape_matches(m, s, &BTreeSet::new(), &impls_of, &mut cache)
            });
            if ok_path && ok_shape {
                ports.insert(s.id.clone());
            }
        }
    }

    // Pass 2: everything.
    let mut by_symbol: BTreeMap<String, Classification> = BTreeMap::new();
    let mut unmatched = Vec::new();
    let mut ambiguities = Vec::new();

    let classifiable = |s: &Symbol| !matches!(s.kind, SymbolKind::Module) && s.parent.is_none();

    for s in model.symbols.iter().filter(|s| classifiable(s)) {
        if s.is_test {
            by_symbol.insert(
                s.id.clone(),
                Classification {
                    symbol: s.id.clone(),
                    pattern: "test".into(),
                    layer: "test".into(),
                    inherited: false,
                    ambiguous: false,
                },
            );
            continue;
        }

        // Candidates match on path and shape; `externs` then decides
        // between the ones the glossary separates by what they talk to.
        let mut candidates: Vec<&Pattern> = Vec::new();
        for p in &cfg.patterns {
            let ok_path = p
                .path
                .as_ref()
                .is_none_or(|x| path_matches(x, &s.file, &mut cache));
            let ok_shape = p
                .symbol
                .as_ref()
                .is_none_or(|m| shape_matches(m, s, &ports, &impls_of, &mut cache));
            if ok_path && ok_shape {
                candidates.push(p);
            }
        }
        if candidates.is_empty() {
            unmatched.push(Unmatched {
                symbol: s.id.clone(),
                kind: s.kind,
                file: s.file.clone(),
                line: s.line,
                name: s.name.clone(),
            });
            continue;
        }

        let reach = extern_reach(model, &s.id);
        let decided: Vec<&&Pattern> = candidates
            .iter()
            .filter(|p| {
                p.externs.is_empty()
                    || p.externs
                        .iter()
                        .any(|want| reach.iter().any(|got| got.starts_with(want.as_str())))
            })
            .collect();

        let group: Option<&str> = candidates
            .first()
            .and_then(|p| p.group.as_deref())
            .filter(|g| candidates.iter().all(|p| p.group.as_deref() == Some(*g)));

        let (pattern, layer, ambiguous) = match (decided.len(), group) {
            (1, _) => {
                let p = decided[0];
                (p.id.clone(), p.layer.clone(), false)
            }
            (_, Some(g)) => {
                // Matched both extern sets, or neither. Say so.
                ambiguities.push(s.id.clone());
                (g.to_string(), candidates[0].layer.clone(), true)
            }
            _ => {
                let p = decided.first().copied().unwrap_or(&candidates[0]);
                (p.id.clone(), p.layer.clone(), false)
            }
        };

        by_symbol.insert(
            s.id.clone(),
            Classification {
                symbol: s.id.clone(),
                pattern,
                layer,
                inherited: false,
                ambiguous,
            },
        );
    }

    // Methods inherit their type. A method is part of its type, not a
    // separate architectural unit, and counting it separately would let a
    // 48-method god object dominate the coverage figure.
    let inherited: Vec<Classification> = model
        .symbols
        .iter()
        .filter_map(|s| {
            let parent = s.parent.as_deref()?;
            let c = by_symbol.get(parent)?;
            Some(Classification {
                symbol: s.id.clone(),
                pattern: c.pattern.clone(),
                layer: c.layer.clone(),
                inherited: true,
                ambiguous: false,
            })
        })
        .collect();
    for c in inherited {
        by_symbol.insert(c.symbol.clone(), c);
    }

    let total = model.symbols.iter().filter(|s| classifiable(s)).count();
    let classified = total - unmatched.len();

    let mut by_pattern: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_layer: BTreeMap<String, usize> = BTreeMap::new();
    for c in by_symbol.values().filter(|c| !c.inherited) {
        *by_pattern.entry(c.pattern.clone()).or_default() += 1;
        *by_layer.entry(c.layer.clone()).or_default() += 1;
    }

    // Group the gaps so the report is a to-do list, not a dump.
    let mut gaps: BTreeMap<String, (usize, String)> = BTreeMap::new();
    for u in &unmatched {
        let dir = match u.file.rfind('/') {
            Some(i) => &u.file[..i],
            None => "src/ root",
        };
        let e = gaps
            .entry(format!("{:?} in {dir}", u.kind).to_lowercase())
            .or_insert((0, format!("{}:{} {}", u.file, u.line, u.name)));
        e.0 += 1;
    }
    let mut gaps: Vec<(String, usize, String)> =
        gaps.into_iter().map(|(k, (n, e))| (k, n, e)).collect();
    gaps.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    Classified {
        by_symbol,
        unmatched,
        coverage: Coverage {
            classified,
            total,
            ports: ports.len(),
            by_pattern,
            by_layer,
            gaps,
        },
        ambiguities,
    }
}
