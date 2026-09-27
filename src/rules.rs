//! `patterns-rust` made executable. See `specs/04-conformance-rules.md`.
//!
//! The glossary does one thing no architecture tool does: every pattern
//! page ends with an **Enforcement** section naming the mechanism that
//! makes the rule hold — compiler, trait, visibility, lint, convention.
//! That axis is first-class here, because it answers a question nothing
//! else answers: *which of my rules is anything actually enforcing?*
//!
//! Two invariants run through this file:
//!
//! * A rule whose premise is absent reports `Unevaluable`, never `Pass`.
//!   Against a crate with no ports, five green checks would be the most
//!   misleading thing this tool could print.
//! * A `Heuristic` rule never gates, under any flag. A ratchet that fails
//!   CI on a guess is switched off within a week, and takes the decidable
//!   rules with it.

use crate::classify::{Classified, Config};
use crate::model::{EdgeKind, EdgeTarget, Model, Symbol, SymbolKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tier {
    /// The build rejects it. Only reachable under `crate-graph.md` Option B.
    Compiler,
    /// The type system carries it — a bound rather than a concrete.
    Trait,
    Visibility,
    Lint,
    Convention,
    /// Already covered by a lint the toolchain ships. Listed, not checked.
    Clippy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// The model contains enough to be sure.
    Decidable,
    /// A smell with a real false-positive rate.
    Heuristic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    Pass,
    Violated,
    /// The premise does not exist in this codebase.
    Unevaluable,
    /// The rule found the architecture working, and counted it.
    Upheld,
    /// Guaranteed elsewhere. Checking it again would overstate this tool's
    /// contribution.
    Delegated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Tone {
    Good,
    Note,
    Warn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub from: String,
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub to: String,
    pub file: String,
    pub line: u32,
    pub detail: String,
}

// `&'static str` for the rule identity: these are compiled-in, not data.
// That costs `Deserialize`, which nothing needs — spec 05's baseline
// stores fingerprints, not findings.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub id: &'static str,
    pub title: &'static str,
    pub source: &'static str,
    pub tier: Tier,
    pub kind: Kind,
    /// Whether a violation should fail the build. Always false for a
    /// heuristic — not by configuration, by construction.
    pub gate: bool,
    pub status: Status,
    pub tone: Tone,
    /// Why the rule could not be evaluated, when it could not be.
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub because: String,
    pub instances: Vec<Instance>,
}

impl Finding {
    /// Whether a violation of this rule is allowed to change the exit code.
    ///
    /// This is the one definition, and the reason it exists is that four places
    /// used to answer the question four ways: `baseline::gating_rules` read
    /// `gate` alone, `baseline::current` excluded heuristics and delegated
    /// rules by hand, `Scorecard::enforcement` counted them with a third
    /// combination, and the CLI's advisory list used a fourth. They agreed only
    /// because the rule table happened to be consistent, and a rule table is
    /// edited by adding to it.
    ///
    /// A heuristic cannot gate — that is what makes it a heuristic, so this is
    /// not a policy choice that could be configured away. A delegated rule is
    /// guaranteed by something outside this tool, so a violation reported here
    /// is information rather than a verdict.
    pub fn gates(&self) -> bool {
        self.gate && self.kind == Kind::Decidable && self.status != Status::Delegated
    }

    fn new(
        id: &'static str,
        title: &'static str,
        source: &'static str,
        tier: Tier,
        kind: Kind,
        gate: bool,
    ) -> Self {
        Finding {
            id,
            title,
            source,
            tier,
            kind,
            gate: gate && kind == Kind::Decidable,
            status: Status::Pass,
            tone: Tone::Good,
            because: String::new(),
            instances: Vec::new(),
        }
    }
    fn unevaluable(mut self, why: &str) -> Self {
        self.status = Status::Unevaluable;
        self.tone = Tone::Note;
        self.because = why.into();
        self
    }
    fn with(mut self, v: Vec<Instance>) -> Self {
        if !v.is_empty() {
            self.status = Status::Violated;
            self.tone = if self.gate { Tone::Warn } else { Tone::Note };
        }
        self.instances = v;
        self
    }
    fn upheld(mut self, v: Vec<Instance>) -> Self {
        self.status = Status::Upheld;
        self.tone = Tone::Good;
        self.instances = v;
        self
    }
    pub fn failed(&self) -> bool {
        self.gates() && self.status == Status::Violated
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub findings: Vec<Finding>,
    pub rules: usize,
    pub compiler_enforced: usize,
    pub checked_here: usize,
    pub advisory: usize,
}

impl Report {
    pub fn violations(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.status == Status::Violated)
            .map(|f| f.instances.len().max(1))
            .sum()
    }
    pub fn gating_failures(&self) -> usize {
        self.findings.iter().filter(|f| f.failed()).count()
    }
    pub fn unevaluable(&self) -> Vec<&Finding> {
        self.findings
            .iter()
            .filter(|f| f.status == Status::Unevaluable)
            .collect()
    }
}

// ------------------------------------------------------------------ lookup

struct Ctx<'a> {
    m: &'a Model,
    cfg: &'a Config,
    c: &'a Classified,
    by_id: BTreeMap<&'a str, &'a Symbol>,
    /// The pattern that owns a symbol, following methods to their type.
    owner_pattern: BTreeMap<&'a str, &'a str>,
    owner_layer: BTreeMap<&'a str, &'a str>,
    /// Directory prefix -> layer, longest first. The layer *rules* are
    /// worded by location — "no symbol in `domain/` reaches an I/O crate" —
    /// and a symbol that matched no pattern still sits in a directory.
    /// Without this, an unclassified symbol escapes every layer rule,
    /// which is precisely the symbol most likely to be doing something
    /// nobody named.
    layer_dirs: Vec<(&'a str, &'a str)>,
    ports: Vec<&'a Symbol>,
}

impl<'a> Ctx<'a> {
    fn new(m: &'a Model, cfg: &'a Config, c: &'a Classified) -> Self {
        let by_id: BTreeMap<&str, &Symbol> = m.symbols.iter().map(|s| (s.id.as_str(), s)).collect();
        let owner_pattern = c
            .by_symbol
            .iter()
            .map(|(k, v)| (k.as_str(), v.pattern.as_str()))
            .collect();
        let owner_layer = c
            .by_symbol
            .iter()
            .map(|(k, v)| (k.as_str(), v.layer.as_str()))
            .collect();
        // A trait's methods inherit its classification (spec 03), so
        // filtering on the pattern alone treats every method of a port as
        // a port in its own right.
        let ports = m
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Trait && c.pattern_of(&s.id) == Some("port"))
            .collect();
        let mut layer_dirs: Vec<(&str, &str)> = cfg
            .patterns
            .iter()
            .filter_map(|p| {
                let path = p.path.as_deref()?;
                // Regex paths name a file, not a directory.
                (!path.starts_with('^')).then_some((path, p.layer.as_str()))
            })
            .collect();
        layer_dirs.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));
        layer_dirs.dedup();
        Ctx {
            m,
            cfg,
            c,
            by_id,
            owner_pattern,
            owner_layer,
            layer_dirs,
            ports,
        }
    }

    /// The classified layer only.
    fn layer(&self, id: &str) -> Option<&str> {
        self.owner_layer.get(id).copied()
    }

    /// The classified layer, or the layer that owns the directory the
    /// symbol sits in. Used by every rule whose wording is about location.
    fn where_(&self, id: &str) -> Option<&str> {
        if let Some(l) = self.owner_layer.get(id) {
            return Some(l);
        }
        let s = self.by_id.get(id)?;
        self.layer_dirs
            .iter()
            .find(|(p, _)| s.file.starts_with(p))
            .map(|(_, l)| *l)
    }
    fn pattern(&self, id: &str) -> Option<&str> {
        self.owner_pattern.get(id).copied()
    }
    fn sym(&self, id: &str) -> Option<&&Symbol> {
        self.by_id.get(id)
    }
    fn any_in_layer(&self, layer: &str) -> bool {
        self.m
            .symbols
            .iter()
            .any(|s| self.where_(&s.id) == Some(layer))
    }
    fn extern_group(&self, name: &str) -> &[String] {
        self.cfg.externs.get(name).map(Vec::as_slice).unwrap_or(&[])
    }
    /// Methods declared on a trait or type.
    fn methods_of(&self, id: &str) -> Vec<&Symbol> {
        self.m
            .symbols
            .iter()
            .filter(|s| s.parent.as_deref() == Some(id))
            .collect()
    }
    fn inst(&self, s: &Symbol, to: &str, detail: String) -> Instance {
        Instance {
            from: s.id.clone(),
            to: to.into(),
            file: s.file.clone(),
            line: s.line,
            detail,
        }
    }
}

fn matches_any(path: &str, group: &[String]) -> bool {
    group.iter().any(|g| path.starts_with(g.as_str()))
}

// ------------------------------------------------------------------- entry

pub fn check(m: &Model, cfg: &Config, c: &Classified) -> Report {
    let x = Ctx::new(m, cfg, c);
    let findings = vec![
        domain_no_io(&x),
        layer_reference(
            &x,
            "domain-no-application",
            "no domain/ symbol references application/",
            "domain",
            "application",
        ),
        layer_reference(
            &x,
            "infra-no-application",
            "no infra/ symbol references application/",
            "infrastructure",
            "application",
        ),
        concrete_outside_root(&x),
        call_through_port(&x),
        flow_skip(&x),
        no_same_level(&x),
        port_signature_purity(&x),
        no_global_mut(&x),
        dto_in_domain(&x),
        use_case_verb(&x),
        time_injected(&x),
        converter_is_pure(&x),
        port_has_fake(&x),
        mixed_layer_module(&x),
        dependency_inversion(&x),
        panic_in_domain(&x),
        // Heuristics. None of these gate.
        store_decides(&x),
        handler_decides(&x),
        service_wraps_one(&x),
        fat_trait(&x),
        renders_off_store(&x),
    ];

    let rules = findings.len();
    let compiler_enforced = findings.iter().filter(|f| f.tier == Tier::Compiler).count();
    let checked_here = findings
        .iter()
        .filter(|f| f.gate && f.status != Status::Delegated)
        .count();
    let advisory = rules - compiler_enforced - checked_here;
    Report {
        findings,
        rules,
        compiler_enforced,
        checked_here,
        advisory,
    }
}

// ----------------------------------------------------------- layer rules

/// The three rows `dependency-hierarchy.md` marks **compiler**-enforced.
/// That is true only under `crate-graph.md` Option B, a workspace split
/// where `domain` is a crate that cannot depend on `rusqlite`. Under
/// Option A — which both target crates are — the same page prescribes
/// "visibility + a source-scan test", and this is that test.
fn tier_for_layer_rule(cfg: &Config) -> Tier {
    match cfg.crate_graph.as_str() {
        "B" => Tier::Compiler,
        _ => Tier::Lint,
    }
}

fn domain_no_io(x: &Ctx) -> Finding {
    let f = Finding::new(
        "domain-no-io",
        "no domain/ symbol reaches an I/O or rendering crate",
        "dependency-hierarchy.md, port.md",
        tier_for_layer_rule(x.cfg),
        Kind::Decidable,
        true,
    );
    if !x.any_in_layer("domain") {
        return f.unevaluable("no symbol classifies as domain");
    }
    if f.tier == Tier::Compiler {
        let mut f = f;
        f.status = Status::Delegated;
        f.because = "crate_graph = \"B\": the build rejects this import".into();
        return f;
    }
    let io: Vec<String> = x
        .extern_group("io")
        .iter()
        .chain(x.extern_group("render"))
        .cloned()
        .collect();
    let mut out = Vec::new();
    for e in &x.m.edges {
        let EdgeTarget::Extern { path, .. } = &e.to else {
            continue;
        };
        if x.where_(&e.from) != Some("domain") || !matches_any(path, &io) {
            continue;
        }
        let Some(s) = x.sym(&e.from) else { continue };
        out.push(Instance {
            from: e.from.clone(),
            to: path.clone(),
            file: e.file.clone(),
            line: e.line,
            detail: format!("{} reaches {}", s.name, path),
        });
    }
    f.with(out)
}

fn layer_reference(
    x: &Ctx,
    id: &'static str,
    title: &'static str,
    from_layer: &str,
    to_layer: &str,
) -> Finding {
    let f = Finding::new(
        id,
        title,
        "dependency-hierarchy.md",
        tier_for_layer_rule(x.cfg),
        Kind::Decidable,
        true,
    );
    if !x.any_in_layer(from_layer) || !x.any_in_layer(to_layer) {
        return f.unevaluable(&format!(
            "no symbol classifies as {}",
            if x.any_in_layer(from_layer) {
                to_layer
            } else {
                from_layer
            }
        ));
    }
    let mut out = Vec::new();
    for e in &x.m.edges {
        let EdgeTarget::Local { id: to } = &e.to else {
            continue;
        };
        // `impl Trait for Type` runs backwards by design; it is the
        // dependency inversion, and it has its own rule.
        if e.kind == EdgeKind::Impl {
            continue;
        }
        if x.where_(&e.from) == Some(from_layer) && x.where_(to) == Some(to_layer) {
            out.push(Instance {
                from: e.from.clone(),
                to: to.clone(),
                file: e.file.clone(),
                line: e.line,
                detail: format!("{} -> {}", e.from, to),
            });
        }
    }
    f.with(out)
}

// --------------------------------------------------------- the port seam

fn concrete_outside_root(x: &Ctx) -> Finding {
    let f = Finding::new(
        "concrete-outside-root",
        "no concrete store or adapter is named outside infra/ and the composition root",
        "port.md — \"the defining violation\"",
        Tier::Lint,
        Kind::Decidable,
        true,
    );
    let concretes: BTreeSet<&str> =
        x.c.by_symbol
            .values()
            .filter(|c| matches!(c.pattern.as_str(), "store" | "adapter" | "infra-impl"))
            .filter(|c| !c.inherited)
            .map(|c| c.symbol.as_str())
            .collect();
    if concretes.is_empty() {
        return f.unevaluable("no concrete store or adapter exists");
    }
    let mut out = Vec::new();
    for e in &x.m.edges {
        let EdgeTarget::Local { id: to } = &e.to else {
            continue;
        };
        if !concretes.contains(to.as_str()) || e.kind == EdgeKind::Impl {
            continue;
        }
        let from_layer = x.where_(&e.from);
        if matches!(
            from_layer,
            Some("infrastructure") | Some("composition-root") | Some("test")
        ) {
            continue;
        }
        out.push(Instance {
            from: e.from.clone(),
            to: to.clone(),
            file: e.file.clone(),
            line: e.line,
            detail: format!("{} names the concrete {to}", e.from),
        });
    }
    f.with(out)
}

fn call_through_port(x: &Ctx) -> Finding {
    let f = Finding::new(
        "call-through-port",
        "callers take &impl Port, never a concrete",
        "port.md rule 2",
        Tier::Trait,
        Kind::Decidable,
        true,
    );
    if x.ports.is_empty() {
        return f.unevaluable("no port trait exists");
    }
    let concretes: BTreeSet<&str> =
        x.c.by_symbol
            .values()
            .filter(|c| matches!(c.pattern.as_str(), "store" | "adapter" | "infra-impl"))
            .map(|c| c.symbol.as_str())
            .collect();
    let mut out = Vec::new();
    for s in &x.m.symbols {
        if !matches!(
            x.pattern(&s.id),
            Some("use-case") | Some("service") | Some("command-handler")
        ) {
            continue;
        }
        let Some(sig) = &s.sig else { continue };
        for p in &sig.params {
            if !p.bounds.is_empty() {
                continue; // `&impl Port` — this is the architecture working.
            }
            for want in &p.ty_paths {
                let hit = concretes
                    .iter()
                    .find(|c| c.rsplit("::").next() == want.rsplit("::").next());
                if let Some(c) = hit {
                    out.push(x.inst(
                        s,
                        c,
                        format!("parameter `{}: {}` names a concrete", p.name, p.ty),
                    ));
                }
            }
        }
    }
    f.with(out)
}

fn port_signature_purity(x: &Ctx) -> Finding {
    let f = Finding::new(
        "port-signature-purity",
        "no port method signature names a non-domain type",
        "port.md",
        Tier::Trait,
        Kind::Decidable,
        true,
    );
    if x.ports.is_empty() {
        return f.unevaluable("no port trait exists");
    }
    let mut out = Vec::new();
    for p in &x.ports {
        for me in x.methods_of(&p.id) {
            let leak: Vec<&str> =
                x.m.edges
                    .iter()
                    .filter(|e| e.from == me.id && e.kind == EdgeKind::Sig)
                    .filter_map(|e| match &e.to {
                        EdgeTarget::Extern { path, .. } => Some(path.as_str()),
                        _ => None,
                    })
                    .filter(|path| !path.starts_with("std::") && !path.starts_with("core::"))
                    .collect();
            for l in leak {
                out.push(x.inst(me, l, format!("`{}` exposes {l}", me.name)));
            }
        }
    }
    f.with(out)
}

fn port_has_fake(x: &Ctx) -> Finding {
    let f = Finding::new(
        "port-has-fake",
        "every port has a fake implementation",
        "testing.md — \"one fake per port\"",
        Tier::Convention,
        Kind::Decidable,
        false,
    );
    if x.ports.is_empty() {
        return f.unevaluable("no port trait exists");
    }
    let mut out = Vec::new();
    for p in &x.ports {
        let impls: Vec<_> =
            x.m.impls
                .iter()
                .filter(|b| b.trait_id.as_deref() == Some(p.id.as_str()))
                .collect();
        if impls.is_empty() {
            out.push(x.inst(p, "", format!("`{}` is a dead port: no impl", p.name)));
            continue;
        }
        let fake = impls
            .iter()
            .any(|b| b.is_test || b.file.ends_with("mem.rs") || b.file.contains("/fake"));
        if !fake {
            out.push(x.inst(
                p,
                "",
                format!(
                    "`{}` has {} impl(s), none of them a fake",
                    p.name,
                    impls.len()
                ),
            ));
        }
    }
    let mut f = f.with(out);
    if f.status == Status::Violated {
        f.tone = Tone::Warn; // warns, does not fail
    }
    f
}

/// Not a violation to tolerate — the architecture succeeding, counted.
fn dependency_inversion(x: &Ctx) -> Finding {
    let f = Finding::new(
        "dependency-inversion",
        "ports are defined in the domain and implemented in infrastructure",
        "dependency-hierarchy.md rule 3",
        Tier::Trait,
        Kind::Decidable,
        true,
    );
    if x.ports.is_empty() {
        return f.unevaluable("no port trait exists");
    }
    let mut good = Vec::new();
    let mut bad = Vec::new();
    for b in &x.m.impls {
        let (Some(t), Some(ty)) = (&b.trait_id, &b.type_id) else {
            continue;
        };
        let tl = x.layer(t);
        let cl = x.layer(ty);
        if tl == Some("domain") && matches!(cl, Some("infrastructure") | Some("test")) {
            good.push(Instance {
                from: ty.clone(),
                to: t.clone(),
                file: b.file.clone(),
                line: b.line,
                detail: format!("impl {} for {}", b.trait_path, b.type_path),
            });
        } else if matches!(tl, Some("infrastructure")) && cl == Some("domain") {
            // The domain implementing infrastructure's interface inverts
            // the inversion.
            bad.push(Instance {
                from: ty.clone(),
                to: t.clone(),
                file: b.file.clone(),
                line: b.line,
                detail: format!("domain type implements infra trait {}", b.trait_path),
            });
        }
    }
    if !bad.is_empty() {
        return f.with(bad);
    }
    f.upheld(good)
}

// ------------------------------------------------------------- flow rules

/// Spec 04 derived this as "an edge jumping >= 1 station in `flow`". That
/// derivation contradicts the glossary it comes from. The dependency
/// matrix in `dependency-hierarchy.md` permits every downward pair above
/// the seam — a command-handler may call a use-case (skipping `service`)
/// and may call a port directly (skipping two). Read literally, the rule
/// fires on the conforming fixture.
///
/// The flow is not a pipeline; it is a partial order with one seam in it.
/// `port` is a permitted terminus from anywhere above, and the violation
/// is reaching *past* the port to what lies below it.
fn flow_skip(x: &Ctx) -> Finding {
    let f = Finding::new(
        "flow-skip",
        "no edge reaches past the port to what implements it",
        "dependency-hierarchy.md rule 2",
        Tier::Lint,
        Kind::Decidable,
        true,
    );
    let flow = &x.cfg.flow;
    let Some(seam) = flow.iter().position(|s| s == "port") else {
        return f.unevaluable("`flow` declares no port station");
    };
    let station: BTreeMap<&str, usize> = flow
        .iter()
        .enumerate()
        .map(|(i, s)| (s.as_str(), i))
        .collect();
    let above =
        x.c.by_symbol
            .values()
            .any(|c| station.get(c.pattern.as_str()).is_some_and(|i| *i < seam));
    let below =
        x.c.by_symbol
            .values()
            .any(|c| station.get(c.pattern.as_str()).is_some_and(|i| *i > seam));
    if !above || !below {
        return f.unevaluable("no station exists on both sides of the port seam");
    }
    let mut out = Vec::new();
    for e in &x.m.edges {
        let EdgeTarget::Local { id: to } = &e.to else {
            continue;
        };
        if e.kind == EdgeKind::Impl {
            continue; // the inversion arrow; checked separately
        }
        let (Some(a), Some(b)) = (x.pattern(&e.from), x.pattern(to)) else {
            continue;
        };
        let (Some(&i), Some(&j)) = (station.get(a), station.get(b)) else {
            continue;
        };
        if i < seam && j > seam {
            out.push(Instance {
                from: e.from.clone(),
                to: to.clone(),
                file: e.file.clone(),
                line: e.line,
                detail: format!("{a} reaches {b} directly, bypassing the port"),
            });
        }
    }
    f.with(out)
}

fn no_same_level(x: &Ctx) -> Finding {
    let f = Finding::new(
        "no-same-level",
        "patterns at the same level do not call each other",
        "dependency-hierarchy.md rule 1",
        Tier::Convention,
        Kind::Decidable,
        true,
    );
    const LEVELS: [&str; 4] = ["use-case", "service", "store", "command-handler"];
    let populated: Vec<&str> = LEVELS
        .iter()
        .copied()
        .filter(|p| x.c.by_symbol.values().any(|c| c.pattern == *p))
        .collect();
    if populated.is_empty() {
        return f.unevaluable("no use-case, service, store or handler exists");
    }
    let mut out = Vec::new();
    for e in x.m.edges.iter().filter(|e| e.kind == EdgeKind::Call) {
        let EdgeTarget::Local { id: to } = &e.to else {
            continue;
        };
        if e.from == *to {
            continue;
        }
        let (Some(a), Some(b)) = (x.pattern(&e.from), x.pattern(to)) else {
            continue;
        };
        if a == b && LEVELS.contains(&a) {
            out.push(Instance {
                from: e.from.clone(),
                to: to.clone(),
                file: e.file.clone(),
                line: e.line,
                detail: format!("{a} calls {a}: move the orchestration up one level"),
            });
        }
    }
    f.with(out)
}

// ------------------------------------------------------- always evaluable

fn no_global_mut(x: &Ctx) -> Finding {
    let f = Finding::new(
        "no-global-mut",
        "no global mutable state",
        "design-philosophy.md principle 3",
        Tier::Convention,
        Kind::Decidable,
        true,
    );
    const CELLS: [&str; 8] = [
        "Mutex", "RwLock", "OnceCell", "OnceLock", "LazyLock", "RefCell", "Cell", "Atomic",
    ];
    let mut out = Vec::new();
    for s in x.m.symbols.iter().filter(|s| s.kind == SymbolKind::Static) {
        let Some(ty) = s.fields.first().map(|f| f.ty.as_str()) else {
            continue;
        };
        if s.is_test {
            continue;
        }
        let why = if ty.starts_with("mut ") {
            Some("static mut".to_string())
        } else {
            CELLS
                .iter()
                .find(|c| ty.contains(*c))
                .map(|c| format!("static holding {c}"))
        };
        if let Some(why) = why {
            out.push(x.inst(s, "", format!("{}: {why} — `{ty}`", s.name)));
        }
    }
    f.with(out)
}

fn dto_in_domain(x: &Ctx) -> Finding {
    let f = Finding::new(
        "dto-in-domain",
        "the domain vocabulary has no Dto in it",
        "naming.md",
        Tier::Lint,
        Kind::Decidable,
        true,
    );
    if !x.any_in_layer("domain") {
        return f.unevaluable("no symbol classifies as domain");
    }
    let banned = &x.cfg.naming.forbid_in_domain;
    let mut out = Vec::new();
    for s in &x.m.symbols {
        if x.where_(&s.id) != Some("domain") {
            continue;
        }
        if banned.iter().any(|b| s.name.ends_with(b.as_str())) {
            out.push(x.inst(
                s,
                "",
                format!("`{}` is named for a transport layer", s.name),
            ));
        }
    }
    f.with(out)
}

/// Evaluated over everything in `domain/use_cases/`, not over what the
/// classifier already called a use-case — the classifier matches on the
/// verb, so checking its output would be circular and could never fire.
fn use_case_verb(x: &Ctx) -> Finding {
    let f = Finding::new(
        "use-case-verb",
        "every use-case is named for the operation it performs",
        "naming.md",
        Tier::Lint,
        Kind::Decidable,
        true,
    );
    let in_dir: Vec<&Symbol> =
        x.m.symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Fn && s.file.contains("domain/use_cases/"))
            .filter(|s| !s.is_test)
            .collect();
    if in_dir.is_empty() {
        return f.unevaluable("no domain/use_cases/ directory");
    }
    let mut out = Vec::new();
    for s in in_dir {
        if x.pattern(&s.id) != Some("use-case") {
            out.push(x.inst(
                s,
                "",
                format!("`{}` does not begin with an allowlisted verb", s.name),
            ));
        }
    }
    f.with(out)
}

fn time_injected(x: &Ctx) -> Finding {
    let f = Finding::new(
        "time-injected",
        "the clock is a port, not a call",
        "use-case.md, testing.md",
        Tier::Convention,
        Kind::Decidable,
        true,
    );
    let group = x.extern_group("time");
    if group.is_empty() {
        return f.unevaluable("no [externs] time group configured");
    }
    let mut out = Vec::new();
    for e in &x.m.edges {
        let EdgeTarget::Extern { path, .. } = &e.to else {
            continue;
        };
        if !matches_any(path, group) {
            continue;
        }
        if matches!(x.where_(&e.from), Some("infrastructure") | Some("test")) {
            continue;
        }
        out.push(Instance {
            from: e.from.clone(),
            to: path.clone(),
            file: e.file.clone(),
            line: e.line,
            detail: format!("{} calls {path} directly", e.from),
        });
    }
    f.with(out)
}

fn converter_is_pure(x: &Ctx) -> Finding {
    let f = Finding::new(
        "converter-is-pure",
        "a converter takes no port, returns no Result, and does no I/O",
        "converter.md",
        Tier::Convention,
        Kind::Decidable,
        true,
    );
    let convs: Vec<&Symbol> =
        x.m.symbols
            .iter()
            .filter(|s| x.pattern(&s.id) == Some("converter"))
            .collect();
    if convs.is_empty() {
        return f.unevaluable("no symbol classifies as converter");
    }
    let io = x.extern_group("io");
    let mut out = Vec::new();
    for s in convs {
        if let Some(sig) = &s.sig {
            if !sig.params.iter().all(|p| p.bounds.is_empty()) {
                out.push(x.inst(s, "", format!("`{}` takes a port", s.name)));
            }
            if sig.ret.as_deref().is_some_and(|r| r.starts_with("Result")) {
                out.push(x.inst(s, "", format!("`{}` returns Result", s.name)));
            }
        }
        let dirty = x.m.edges.iter().any(|e| {
            e.from == s.id
                && matches!(&e.to, EdgeTarget::Extern { path, .. } if matches_any(path, io))
        });
        if dirty {
            out.push(x.inst(s, "", format!("`{}` reaches an I/O crate", s.name)));
        }
    }
    f.with(out)
}

fn mixed_layer_module(x: &Ctx) -> Finding {
    let f = Finding::new(
        "mixed-layer-module",
        "a module holds symbols from one layer",
        "design-philosophy.md, spec 02 detector 2",
        Tier::Convention,
        Kind::Decidable,
        true,
    );
    // One classified symbol cannot produce a mixed module, and reporting
    // a pass on that basis claims the modules were examined.
    let per_module =
        x.c.by_symbol
            .values()
            .filter(|c| !c.inherited)
            .filter_map(|c| x.sym(&c.symbol).map(|s| s.module.as_str()))
            .fold(BTreeMap::<&str, usize>::new(), |mut a, m| {
                *a.entry(m).or_default() += 1;
                a
            });
    if !per_module.values().any(|n| *n >= 2) {
        return f.unevaluable("no module has two classified symbols to compare");
    }
    let out: Vec<Instance> =
        x.c.mixed_layer_modules(x.m)
            .into_iter()
            .map(|(module, layers)| {
                let file =
                    x.m.modules
                        .iter()
                        .find(|md| md.id == module)
                        .map(|md| md.file.clone())
                        .unwrap_or_default();
                let l: Vec<&str> = layers.iter().map(String::as_str).collect();
                Instance {
                    from: module.clone(),
                    to: String::new(),
                    file,
                    line: 1,
                    detail: format!("{module} spans {}", l.join(" + ")),
                }
            })
            .collect();
    f.with(out)
}

/// The domain should not panic, and nothing here checks that.
///
/// This used to be `Status::Delegated` with the reason "clippy::unwrap_used /
/// expect_used, denied per-module in clippy.toml". Neither half was true: there
/// is no `clippy.toml` in this repository, and `clippy.toml` configures lint
/// thresholds rather than lint levels, so it cannot scope a denial to one
/// module in any case. The rule was delegating to a mechanism that did not
/// exist, in a repository that holds two `unwrap`s of its own.
///
/// It is `Unevaluable` now, which is the honest status: a decidable rule whose
/// premise this tool cannot supply. The model records no edge for `.unwrap()`,
/// so there is nothing to decide from — writing the rule would mean teaching the
/// scanner about panicking calls, which is a different change.
///
/// Two ways to close it, both named so the next person does not have to guess:
///
/// * delegate it for real — `[lints.clippy] unwrap_used = "deny"` in
///   `Cargo.toml` denies it crate-wide, which over-covers but never
///   under-covers;
/// * decide it here — record `unwrap`/`expect` calls in the model, then this
///   becomes an ordinary check over the domain layer.
///
/// Every symbol the classifier calls `domain/`, checked for the calls that
/// abort instead of returning: `unwrap`, `expect`, and their `_err` forms.
///
/// Was `Status::Delegated` on the grounds that clippy ships `unwrap_used` and
/// a `clippy.toml` could deny it per module. Neither half held: this crate
/// configures no clippy lints at all, so nothing was enforcing it, and a rule
/// delegated to nothing has no premise and cannot be evaluated — which left
/// `Status::Unevaluable` and, in turn, `--require-evaluable` red for every
/// crate. Deciding it from `Symbol.panics` costs one comparison and makes the
/// flag usable again.
fn panic_in_domain(x: &Ctx) -> Finding {
    let f = Finding::new(
        "panic-in-domain",
        "the domain does not unwrap",
        "design-philosophy.md",
        Tier::Clippy,
        Kind::Decidable,
        true,
    );
    if !x.any_in_layer("domain") {
        return f.unevaluable("no symbol classifies as domain");
    }
    let mut out = Vec::new();
    for s in &x.m.symbols {
        if x.where_(&s.id) != Some("domain") {
            continue;
        }
        for (method, line) in &s.panics {
            out.push(Instance {
                from: s.id.clone(),
                to: method.clone(),
                file: s.file.clone(),
                line: *line,
                detail: format!("`{}` can abort on {}", s.name, method),
            });
        }
    }
    f.with(out)
}

// ------------------------------------------------------------ heuristics

/// The violation the glossary cares most about, and it is not decidable.
/// "The store is making a business decision" is semantic; the best proxy
/// is a store method returning a domain enum. Reported prominently,
/// gates nothing.
fn store_decides(x: &Ctx) -> Finding {
    let f = Finding::new(
        "store-decides",
        "a store returns a decision instead of data",
        "store.md, use-case.md, service.md",
        Tier::Convention,
        Kind::Heuristic,
        false,
    );
    let stores: Vec<&Symbol> =
        x.m.symbols
            .iter()
            .filter(|s| {
                matches!(
                    x.pattern(&s.id),
                    Some("store") | Some("adapter") | Some("infra-impl")
                )
            })
            .filter(|s| s.kind == SymbolKind::Method)
            .collect();
    if stores.is_empty() {
        return f.unevaluable("no store or adapter method exists");
    }
    let domain_enums: BTreeSet<&str> =
        x.m.symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Enum && x.where_(&s.id) == Some("domain"))
            .map(|s| s.name.as_str())
            .collect();
    let mut out = Vec::new();
    for s in stores {
        let Some(sig) = &s.sig else { continue };
        for p in &sig.ret_paths {
            let leaf = p.rsplit("::").next().unwrap_or(p);
            if domain_enums.contains(leaf) {
                out.push(x.inst(
                    s,
                    leaf,
                    format!("`{}` returns the domain enum {leaf}", s.name),
                ));
            }
        }
        let heads: BTreeSet<&str> = s
            .matches_on
            .iter()
            .map(|mo| mo.split("::").next().unwrap_or(mo))
            .filter(|h| domain_enums.contains(h))
            .collect();
        for head in heads {
            out.push(x.inst(s, head, format!("`{}` branches on {head}", s.name)));
        }
    }
    f.with(out)
}

fn handler_decides(x: &Ctx) -> Finding {
    let f = Finding::new(
        "handler-decides",
        "a command handler branches on domain state",
        "command-handler.md",
        Tier::Convention,
        Kind::Heuristic,
        false,
    );
    let handlers: Vec<&Symbol> =
        x.m.symbols
            .iter()
            .filter(|s| x.pattern(&s.id) == Some("command-handler"))
            .collect();
    if handlers.is_empty() {
        return f.unevaluable("no command-handler exists");
    }
    let domain_enums: BTreeSet<&str> =
        x.m.symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Enum && x.where_(&s.id) == Some("domain"))
            .map(|s| s.name.as_str())
            .collect();
    let mut out = Vec::new();
    for s in handlers {
        // One finding per enum, not one per arm.
        let heads: BTreeSet<&str> = s
            .matches_on
            .iter()
            .map(|mo| mo.split("::").next().unwrap_or(mo))
            .filter(|h| domain_enums.contains(h))
            .collect();
        for head in heads {
            out.push(x.inst(s, head, format!("`{}` matches on {head}", s.name)));
        }
    }
    f.with(out)
}

fn service_wraps_one(x: &Ctx) -> Finding {
    let f = Finding::new(
        "service-wraps-one",
        "a service that wraps a single use-case is ceremony without a workflow",
        "service.md",
        Tier::Convention,
        Kind::Heuristic,
        false,
    );
    let services: Vec<&Symbol> =
        x.m.symbols
            .iter()
            .filter(|s| x.pattern(&s.id) == Some("service"))
            .filter(|s| matches!(s.kind, SymbolKind::Fn | SymbolKind::Method))
            .collect();
    if services.is_empty() {
        return f.unevaluable("no symbol classifies as service");
    }
    let mut out = Vec::new();
    for s in services {
        let calls: Vec<&str> =
            x.m.edges
                .iter()
                .filter(|e| e.from == s.id && e.kind == EdgeKind::Call)
                .filter_map(|e| match &e.to {
                    EdgeTarget::Local { id } => Some(id.as_str()),
                    _ => None,
                })
                .filter(|id| x.pattern(id) == Some("use-case"))
                .collect();
        if calls.len() == 1 {
            out.push(x.inst(
                s,
                calls[0],
                format!("`{}` wraps exactly one use-case", s.name),
            ));
        }
    }
    f.with(out)
}

fn fat_trait(x: &Ctx) -> Finding {
    let f = Finding::new(
        "fat-trait",
        "a port stays narrow enough to fake",
        "port.md — interface segregation",
        Tier::Convention,
        Kind::Heuristic,
        false,
    );
    if x.ports.is_empty() {
        return f.unevaluable("no port trait exists");
    }
    const MAX: usize = 12;
    let mut out = Vec::new();
    for p in &x.ports {
        let n = x.methods_of(&p.id).len();
        if n > MAX {
            out.push(x.inst(
                p,
                "",
                format!("`{}` declares {n} methods (> {MAX})", p.name),
            ));
        }
    }
    f.with(out)
}

fn renders_off_store(x: &Ctx) -> Finding {
    let f = Finding::new(
        "renders-off-store",
        "a render function reads a view-model, not the object graph",
        "view-model.md",
        Tier::Convention,
        Kind::Heuristic,
        false,
    );
    let render: Vec<&Symbol> =
        x.m.symbols
            .iter()
            .filter(|s| matches!(s.kind, SymbolKind::Fn | SymbolKind::Method))
            .filter(|s| !s.is_test)
            .filter(|s| {
                s.name.starts_with("render_")
                    || s.name.starts_with("draw_")
                    || matches!(x.pattern(&s.id), Some("view-model"))
            })
            .collect();
    if render.is_empty() {
        return f.unevaluable("no render function found");
    }
    let mut out = Vec::new();
    for s in render {
        for c in &s.field_chains {
            out.push(x.inst(s, c, format!("`{}` reaches through `{c}`", s.name)));
        }
    }
    f.with(out)
}
