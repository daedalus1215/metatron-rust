//! Cohesion analysis. See `specs/02-cohesion-analysis.md`.
//!
//! Two measures over the same data, answering two different questions:
//!
//! * **LCOM4** — connected components of the bipartite method<->field graph.
//!   *Is this type already several types?* The standard metric, unchanged
//!   since Hitz & Montazeri. Comparable across projects.
//! * **Modularity** — Newman's Q for the best partition found on the
//!   method projection. *Where would it split?* LCOM4 cannot answer this:
//!   a type whose every method touches one shared field has LCOM4 = 1 no
//!   matter how many distinct jobs it is doing.
//!
//! Both are computed from field *access*, never from names. Names are used
//! only to label a component once it has been found. `tests/cohesion.rs`
//! pins that with a fixture whose fields are called `f1..f12`.

use crate::model::{Model, Symbol, SymbolKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Below this many fields there is nothing to partition, and cohesion is
/// not a meaningful question. This is what keeps a store with one
/// connection and twenty-four methods from being called a god object —
/// `enoch::db::Db` is the fixture for it.
const TRIVIAL_FIELDS: usize = 5;

/// A type this wide is worth reporting on whatever its modularity.
const WIDE_FIELDS: usize = 12;

/// *Lack of cohesion of methods* is not a question you can ask of two
/// methods. Below this, a type reports cohesive whatever the graph says —
/// `arioch::Config`, a settings record with two helpers that happen to
/// touch different fields, is otherwise a permanent false positive.
const TRIVIAL_METHODS: usize = 3;

/// Newman's rule of thumb. Above this, the partition is real structure
/// rather than an artefact of the algorithm always returning something.
const STRONG_Q: f64 = 0.30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    /// One job, or too little state to have more than one.
    Cohesive,
    /// LCOM4 > 1: the pieces are already disjoint. The split is free.
    Disconnected,
    /// Wide, and the partition is strong enough to act on.
    Splittable,
    /// Wide, and there is no clean seam. The harder finding of the two:
    /// nothing can be lifted out without dragging shared state with it.
    Tangled,
    /// Method set is incomplete (macro-generated items). Not analysed.
    Excluded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Component {
    /// Label derived from the dominant field-name prefix, or `None` when
    /// the fields have no prefix in common — which is itself a signal that
    /// the component is not a concept anyone has named yet.
    pub name: Option<String>,
    pub fields: Vec<String>,
    pub methods: Vec<String>,
    pub loc: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeCohesion {
    pub symbol: String,
    pub name: String,
    pub file: String,
    pub line: u32,
    pub field_count: usize,
    pub method_count: usize,
    /// Connected components over the full bipartite graph. The textbook
    /// number, reported unmodified so it stays comparable with published
    /// figures.
    pub lcom4: usize,
    /// The same count with dead fields and inert methods removed. Dead
    /// state inflates LCOM4 truthfully but unactionably — a type with one
    /// forgotten field would otherwise always read as two types. The
    /// verdict is taken from this.
    pub lcom4_core: usize,
    pub modularity: f64,
    pub verdict: Verdict,
    pub components: Vec<Component>,
    /// Fields reached from more than one component. These become
    /// constructor parameters of each extracted type, and they are the
    /// reason the split is not free.
    pub shared: Vec<String>,
    /// Accesses that cross a component boundary — the cost of the refactor,
    /// counted before it starts.
    pub cross_edges: usize,
    /// Declared and never touched through `self`.
    pub unused_fields: Vec<String>,
    /// Assigned by some method, read by none, and named nowhere else.
    pub write_only_fields: Vec<String>,
}

/// A function whose extern fan-out spans more than one concern group.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixedConcern {
    pub symbol: String,
    pub file: String,
    pub line: u32,
    pub concerns: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CohesionReport {
    pub types: Vec<TypeCohesion>,
    pub mixed_concern: Vec<MixedConcern>,
}

// ---------------------------------------------------------------- entry

pub fn analyse(model: &Model) -> CohesionReport {
    let tainted: BTreeSet<&str> = model.macro_tainted.iter().map(String::as_str).collect();
    let mut types = Vec::new();

    let read_elsewhere: BTreeSet<&str> = model
        .foreign_field_reads
        .iter()
        .map(String::as_str)
        .collect();

    // Structs only. An enum's fields belong to variants and are disjoint by
    // construction, so LCOM4 over them just recounts the variants; and a
    // union has no methods worth partitioning.
    for t in model
        .symbols
        .iter()
        .filter(|s| matches!(s.kind, SymbolKind::Struct) && !s.is_test)
    {
        let methods: Vec<&Symbol> = model
            .symbols
            .iter()
            .filter(|s| {
                s.parent.as_deref() == Some(t.id.as_str())
                    && !s.is_test
                    // An associated function is not a method. Including
                    // `App::new` would add an isolated vertex to every type
                    // that has a constructor.
                    && s.sig.as_ref().is_some_and(|g| g.takes_self)
            })
            .collect();
        // A struct with no methods is a record, not a unit of behaviour.
        // Cohesion of methods is undefined when there are none, and
        // reporting every field of a config struct as "never touched" is
        // how a tool teaches people to ignore it.
        if methods.is_empty() {
            continue;
        }
        types.push(one_type(
            t,
            &methods,
            &read_elsewhere,
            tainted.contains(t.id.as_str()),
        ));
    }

    types.sort_by(|a, b| {
        b.field_count
            .cmp(&a.field_count)
            .then(b.method_count.cmp(&a.method_count))
            .then(a.symbol.cmp(&b.symbol))
    });

    CohesionReport {
        types,
        mixed_concern: mixed_concern(model),
    }
}

fn one_type(
    t: &Symbol,
    methods: &[&Symbol],
    read_elsewhere: &BTreeSet<&str>,
    tainted: bool,
) -> TypeCohesion {
    let fields: Vec<String> = t.fields.iter().map(|f| f.name.clone()).collect();
    let fset: BTreeSet<&str> = fields.iter().map(String::as_str).collect();
    let mset: BTreeSet<&str> = methods.iter().map(|s| s.name.as_str()).collect();

    // Access sets, restricted to what actually exists on this type. A
    // `self.foo()` naming a trait-provided method is not a vertex here.
    let touch: Vec<BTreeSet<&str>> = methods
        .iter()
        .map(|s| {
            s.self_fields
                .iter()
                .filter_map(|f| fset.get(f.as_str()).copied())
                .collect()
        })
        .collect();
    let calls: Vec<BTreeSet<&str>> = methods
        .iter()
        .map(|s| {
            s.self_calls
                .iter()
                .filter_map(|c| mset.get(c.as_str()).copied())
                .filter(|c| *c != s.name.as_str())
                .collect()
        })
        .collect();

    // Untouched by its own methods *and* named by no field expression
    // anywhere else in the crate. The second half is what a `pub` field
    // needs before it can be called dead — `arioch::App::selected_category`
    // is untouched through `self` and read in `ui.rs`.
    let unused: Vec<String> = fields
        .iter()
        .filter(|f| !touch.iter().any(|t| t.contains(f.as_str())))
        .filter(|f| !read_elsewhere.contains(f.as_str()))
        .cloned()
        .collect();

    let reads: BTreeSet<&str> = methods
        .iter()
        .flat_map(|s| s.self_reads.iter().map(String::as_str))
        .collect();
    let write_only: Vec<String> = fields
        .iter()
        .filter(|f| touch.iter().any(|t| t.contains(f.as_str())))
        .filter(|f| !reads.contains(f.as_str()))
        .filter(|f| !read_elsewhere.contains(f.as_str()))
        .cloned()
        .collect();

    let lcom4 = components_of(&fields, methods, &touch, &calls);
    let live: Vec<String> = fields
        .iter()
        .filter(|f| touch.iter().any(|t| t.contains(f.as_str())))
        .cloned()
        .collect();
    // Inert means no field, no outgoing call, *and* no incoming call.
    // Dropping a stateless private helper that two methods share would
    // split their components apart — the false positive spec 02 names.
    let called: BTreeSet<&str> = calls.iter().flat_map(|c| c.iter().copied()).collect();
    let alive: Vec<bool> = methods
        .iter()
        .enumerate()
        .map(|(i, s)| {
            !touch[i].is_empty() || !calls[i].is_empty() || called.contains(s.name.as_str())
        })
        .collect();
    let keep = |i: &usize| alive[*i];
    let live_m: Vec<&Symbol> = methods
        .iter()
        .enumerate()
        .filter(|(i, _)| keep(i))
        .map(|(_, s)| *s)
        .collect();
    let live_t: Vec<BTreeSet<&str>> = touch
        .iter()
        .enumerate()
        .filter(|(i, _)| keep(i))
        .map(|(_, t)| t.clone())
        .collect();
    let live_c: Vec<BTreeSet<&str>> = calls
        .iter()
        .enumerate()
        .filter(|(i, _)| keep(i))
        .map(|(_, c)| c.clone())
        .collect();
    let lcom4_core = components_of(&live, &live_m, &live_t, &live_c);
    let (q, groups) = communities(methods, &touch, &calls);

    let mut base = TypeCohesion {
        symbol: t.id.clone(),
        name: t.name.clone(),
        file: t.file.clone(),
        line: t.line,
        field_count: fields.len(),
        method_count: methods.len(),
        lcom4,
        lcom4_core,
        modularity: q,
        verdict: Verdict::Cohesive,
        components: Vec::new(),
        shared: Vec::new(),
        cross_edges: 0,
        unused_fields: unused,
        write_only_fields: write_only,
    };

    if tainted {
        // Half a method set produces half an access map, and every field
        // the generated methods touch would report as dead. Say nothing.
        base.verdict = Verdict::Excluded;
        base.unused_fields.clear();
        base.write_only_fields.clear();
        return base;
    }

    base.verdict = if fields.len() < TRIVIAL_FIELDS || methods.len() < TRIVIAL_METHODS {
        Verdict::Cohesive
    } else if lcom4_core > 1 {
        Verdict::Disconnected
    } else if fields.len() >= WIDE_FIELDS {
        if q >= STRONG_Q {
            Verdict::Splittable
        } else {
            Verdict::Tangled
        }
    } else {
        Verdict::Cohesive
    };

    if matches!(base.verdict, Verdict::Cohesive) {
        return base;
    }

    // Attach each field to the group that touches it most (ties to the
    // lexicographically first group, for determinism).
    let mut owner: BTreeMap<&str, usize> = BTreeMap::new();
    for f in &fields {
        let mut best: Option<(usize, usize)> = None;
        for (gi, g) in groups.iter().enumerate() {
            let n = g.iter().filter(|&&mi| touch[mi].contains(f.as_str())).count();
            if n > 0 && best.map_or(true, |(bn, _)| n > bn) {
                best = Some((n, gi));
            }
        }
        if let Some((_, gi)) = best {
            owner.insert(f.as_str(), gi);
        }
    }

    base.components = groups
        .iter()
        .enumerate()
        .map(|(gi, g)| {
            let f: Vec<String> = fields
                .iter()
                .filter(|f| owner.get(f.as_str()) == Some(&gi))
                .cloned()
                .collect();
            Component {
                name: label(&f),
                methods: g.iter().map(|&mi| methods[mi].name.clone()).collect(),
                loc: g.iter().map(|&mi| methods[mi].loc).sum(),
                fields: f,
            }
        })
        // A proposed type with no state is not a type. One stateless
        // method left over by the partition is an artefact, not a seam.
        .filter(|c| !c.fields.is_empty() || c.methods.len() > 1)
        .collect();
    base.components
        .sort_by(|a, b| b.fields.len().cmp(&a.fields.len()).then(a.methods.cmp(&b.methods)));

    let group_of: BTreeMap<usize, usize> = groups
        .iter()
        .enumerate()
        .flat_map(|(gi, g)| g.iter().map(move |&mi| (mi, gi)))
        .collect();

    let mut shared = BTreeSet::new();
    let mut cross = 0usize;
    for (mi, t) in touch.iter().enumerate() {
        let Some(&gi) = group_of.get(&mi) else { continue };
        for f in t {
            match owner.get(f) {
                Some(&fg) if fg != gi => {
                    cross += 1;
                    shared.insert((*f).to_string());
                }
                _ => {}
            }
        }
    }
    base.shared = shared.into_iter().collect();
    base.cross_edges = cross;
    base
}

// ------------------------------------------------------------ LCOM4

/// Connected components of the bipartite graph. Fields touched by no
/// method are singletons; they are reported as unused rather than as
/// components, but they still count, because that is the definition.
fn components_of(
    fields: &[String],
    methods: &[&Symbol],
    touch: &[BTreeSet<&str>],
    calls: &[BTreeSet<&str>],
) -> usize {
    let nf = fields.len();
    let idx_f: BTreeMap<&str, usize> = fields.iter().enumerate().map(|(i, f)| (f.as_str(), i)).collect();
    let idx_m: BTreeMap<&str, usize> = methods
        .iter()
        .enumerate()
        .map(|(i, s)| (s.name.as_str(), nf + i))
        .collect();

    let mut uf = Uf::new(nf + methods.len());
    for (mi, t) in touch.iter().enumerate() {
        for f in t {
            // A vertex can be absent when this runs over the live subgraph:
            // a method that touches nothing is dropped, but a sibling that
            // calls it still names it.
            if let Some(&fi) = idx_f.get(f) {
                uf.union(nf + mi, fi);
            }
        }
    }
    for (mi, c) in calls.iter().enumerate() {
        for other in c {
            if let Some(&oi) = idx_m.get(other) {
                uf.union(nf + mi, oi);
            }
        }
    }
    (0..nf + methods.len())
        .map(|i| uf.find(i))
        .collect::<BTreeSet<_>>()
        .len()
}

struct Uf(Vec<usize>);
impl Uf {
    fn new(n: usize) -> Self {
        Uf((0..n).collect())
    }
    fn find(&mut self, mut x: usize) -> usize {
        while self.0[x] != x {
            self.0[x] = self.0[self.0[x]];
            x = self.0[x];
        }
        x
    }
    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[a.max(b)] = a.min(b);
        }
    }
}

// -------------------------------------------------------- modularity

/// Greedy agglomerative modularity maximisation (Clauset-Newman-Moore) on
/// the method projection, where two methods are joined with weight equal
/// to the number of fields they share, plus one if either calls the other.
///
/// Merges are evaluated in sorted order and accepted only on a strict
/// improvement, so the result does not depend on hash iteration order.
/// Determinism matters: spec 05 puts this output in a baseline file.
fn communities(
    methods: &[&Symbol],
    touch: &[BTreeSet<&str>],
    calls: &[BTreeSet<&str>],
) -> (f64, Vec<Vec<usize>>) {
    let n = methods.len();
    let mut w = vec![BTreeMap::<usize, f64>::new(); n];
    let mut total = 0.0f64;
    for a in 0..n {
        for b in a + 1..n {
            let mut x = touch[a].intersection(&touch[b]).count() as f64;
            if calls[a].contains(methods[b].name.as_str())
                || calls[b].contains(methods[a].name.as_str())
            {
                x += 1.0;
            }
            if x > 0.0 {
                w[a].insert(b, x);
                w[b].insert(a, x);
                total += x;
            }
        }
    }
    if total == 0.0 {
        return (0.0, (0..n).map(|i| vec![i]).collect());
    }

    let deg: Vec<f64> = w.iter().map(|m| m.values().sum()).collect();
    let mut com: Vec<usize> = (0..n).collect();

    loop {
        // Community aggregates, recomputed each round: cheap at these sizes
        // and immune to the drift an incremental version would accumulate.
        let mut dtot: BTreeMap<usize, f64> = BTreeMap::new();
        for i in 0..n {
            *dtot.entry(com[i]).or_default() += deg[i];
        }
        let mut between: BTreeMap<(usize, usize), f64> = BTreeMap::new();
        for a in 0..n {
            for (&b, &x) in &w[a] {
                if a < b && com[a] != com[b] {
                    let k = (com[a].min(com[b]), com[a].max(com[b]));
                    *between.entry(k).or_default() += x;
                }
            }
        }

        // dQ for merging x and y, with Q = sum_c [ L_c/m - (d_c/2m)^2 ].
        let m = total;
        let mut best: Option<(f64, (usize, usize))> = None;
        for (&(x, y), &wxy) in &between {
            let dq = wxy / m - dtot[&x] * dtot[&y] / (2.0 * m * m);
            if dq > 1e-12 && best.map_or(true, |(bq, _)| dq > bq) {
                best = Some((dq, (x, y)));
            }
        }
        let Some((_, (x, y))) = best else { break };
        for c in com.iter_mut() {
            if *c == y {
                *c = x;
            }
        }
    }

    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (i, &c) in com.iter().enumerate() {
        groups.entry(c).or_default().push(i);
    }
    let groups: Vec<Vec<usize>> = groups.into_values().collect();
    (modularity(&com, &w, &deg, total), groups)
}

fn modularity(com: &[usize], w: &[BTreeMap<usize, f64>], deg: &[f64], total: f64) -> f64 {
    let mut lin: BTreeMap<usize, f64> = BTreeMap::new();
    let mut dtot: BTreeMap<usize, f64> = BTreeMap::new();
    for i in 0..com.len() {
        *dtot.entry(com[i]).or_default() += deg[i];
        for (&j, &x) in &w[i] {
            if i < j && com[i] == com[j] {
                *lin.entry(com[i]).or_default() += x;
            }
        }
    }
    dtot.iter()
        .map(|(c, d)| lin.get(c).copied().unwrap_or(0.0) / total - (d / (2.0 * total)).powi(2))
        .sum()
}

// -------------------------------------------------------------- naming

/// Dominant `snake_case` first segment, when one covers a third of the
/// component's fields. Labels only; never an input to the partition.
fn label(fields: &[String]) -> Option<String> {
    if fields.len() < 2 {
        return None;
    }
    let mut count: BTreeMap<&str, usize> = BTreeMap::new();
    for f in fields {
        if let Some((head, _)) = f.split_once('_') {
            *count.entry(head).or_default() += 1;
        }
    }
    let (head, n) = count.into_iter().max_by_key(|&(h, n)| (n, std::cmp::Reverse(h)))?;
    if n < 2 || n * 3 < fields.len() {
        return None;
    }
    let mut c = head.chars();
    Some(c.next()?.to_uppercase().collect::<String>() + c.as_str())
}

// ------------------------------------------------- mixed-concern fns

/// Default concern groups. Spec 03 makes these configurable; until then
/// they are the smallest table that separates the two things arioch mixes.
fn concern_of(krate: &str, path: &str) -> Option<&'static str> {
    // `std` is one crate covering every concern there is, so it has to be
    // grouped by module. Everything else groups by crate.
    if krate == "std" || krate == "core" || krate == "alloc" {
        let m = path.split("::").nth(1)?;
        return match m {
            "fs" | "process" | "net" | "env" => Some("io"),
            _ => None,
        };
    }
    match krate {
        "rusqlite" | "sqlx" | "diesel" | "redis" => Some("persistence"),
        "ratatui" | "crossterm" | "tui" | "termion" => Some("ui"),
        "reqwest" | "hyper" | "ureq" => Some("network"),
        "serde_json" | "toml" | "serde_yaml" => Some("serialization"),
        _ => None,
    }
}

fn mixed_concern(model: &Model) -> Vec<MixedConcern> {
    use crate::model::EdgeTarget;
    let mut by: BTreeMap<&str, BTreeMap<String, BTreeSet<String>>> = BTreeMap::new();
    for e in &model.edges {
        if let EdgeTarget::Extern { krate, path } = &e.to {
            if let Some(c) = concern_of(krate, path) {
                by.entry(e.from.as_str())
                    .or_default()
                    .entry(c.to_string())
                    .or_default()
                    .insert(path.clone());
            }
        }
    }
    let mut out: Vec<MixedConcern> = by
        .into_iter()
        .filter(|(_, c)| c.len() > 1)
        .filter_map(|(id, c)| {
            let s = model.symbol(id)?;
            if !matches!(s.kind, SymbolKind::Fn | SymbolKind::Method) || s.is_test {
                return None;
            }
            Some(MixedConcern {
                symbol: id.to_string(),
                file: s.file.clone(),
                line: s.line,
                concerns: c
                    .into_iter()
                    .map(|(k, v)| (k, v.into_iter().collect()))
                    .collect(),
            })
        })
        .collect();
    out.sort_by(|a, b| b.concerns.len().cmp(&a.concerns.len()).then(a.symbol.cmp(&b.symbol)));
    out
}
