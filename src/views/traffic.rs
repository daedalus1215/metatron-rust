//! `traffic` — CLI and TUI, not HTTP.
//!
//! metatron's most striking view animates a request from route to
//! repository. There are no routes here, but `command-handler.md` names
//! the exact analog:
//!
//! > **CLI:** a `fn cmd_<verb>(store: &impl Store, …)` per clap subcommand.
//! > **TUI:** the event loop dispatches to
//! > `fn handle_<mode>_key(&mut self, store: &impl Store, key)`.
//!
//! So an inbound signal is a CLI subcommand or a TUI key in a mode, and
//! the trace follows `Call` edges through service → use-case → port,
//! ending at the impl that satisfies the port.
//!
//! Two honest limits, both from spec 01: `Call` edges are best-effort, and
//! a TUI key dispatch is a `match` on a `KeyCode`, so "which key reaches
//! which use case" would mean reading match arms. **A trace that cannot be
//! resolved is reported as a diagnostic and not drawn** — metatron's rule
//! that an unparseable route is reported rather than guessed at.

use crate::model::{EdgeKind, EdgeTarget};
use crate::scorecard::Scorecard;
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Serialize)]
pub struct Hop {
    /// Symbol name.
    pub cls: String,
    /// Its pattern — the station it occupies.
    pub kind: String,
    pub method: String,
    pub d: usize,
    pub m: String,
}

#[derive(Serialize)]
pub struct Entry {
    pub id: String,
    /// `cli` or `tui` — metatron's HTTP verb slot.
    pub verb: String,
    /// The subcommand or key handler name, metatron's route slot.
    pub route: String,
    pub module: String,
    pub file: String,
    pub line: u32,
    pub cls: String,
    pub handler: String,
    pub ret: Option<String>,
    /// The clap argument struct, where one is named. metatron's DTO slot.
    pub dto: Option<String>,
    pub params: Vec<String>,
    pub flat: Vec<Hop>,
    /// True when the trace ends at a port rather than a concrete.
    pub through_port: bool,
    pub target: Option<String>,
}

#[derive(Serialize)]
pub struct Traffic {
    #[serde(rename = "generatedAt")]
    pub generated_at: String,
    pub endpoints: Vec<Entry>,
    /// Traces that could not be resolved. Reported, not drawn.
    pub diagnostics: Vec<String>,
}

/// Entry points: a classified command-handler, or a `cmd_*` / `handle_*`
/// free function in a crate that has no classification yet.
pub fn entries(s: &Scorecard) -> Vec<Entry> {
    let mut out = Vec::new();
    for sym in &s.model.symbols {
        if sym.is_test {
            continue;
        }
        let classified = s.classified.pattern_of(&sym.id) == Some("command-handler");
        let by_name = sym.name.starts_with("cmd_")
            || (sym.name.starts_with("handle_") && sym.name.ends_with("_key"));
        if !classified && !by_name {
            continue;
        }
        let verb = if sym.name.starts_with("cmd_") { "cli" } else { "tui" };
        let sig = sym.sig.as_ref();
        let params: Vec<String> = sig
            .map(|g| g.params.iter().map(|p| format!("{}: {}", p.name, p.ty)).collect())
            .unwrap_or_default();
        // The clap struct supplies the argument payload the way a DTO does
        // in the NestJS version.
        let dto = sig.and_then(|g| {
            g.params.iter().find_map(|p| {
                p.ty_paths.iter().find(|t| {
                    let leaf = t.rsplit("::").next().unwrap_or(t);
                    leaf.ends_with("Args") || leaf.ends_with("Command") || leaf.ends_with("Cli")
                })
            })
        });
        let (flat, through_port, target) = trace(s, &sym.id);
        out.push(Entry {
            id: sym.id.clone(),
            verb: verb.into(),
            route: sym.name.clone(),
            module: sym.module.clone(),
            file: sym.file.clone(),
            line: sym.line,
            cls: sym.parent.clone().unwrap_or_else(|| sym.module.clone()),
            handler: sym.name.clone(),
            ret: sig.and_then(|g| g.ret.clone()),
            dto: dto.cloned(),
            params,
            flat,
            through_port,
            target,
        });
    }
    out.sort_by(|a, b| a.verb.cmp(&b.verb).then(a.route.cmp(&b.route)));
    out
}

/// Follow `Call` and `Bound` edges inward, breadth-first, stopping at the
/// port. Depth-limited: a cycle in a best-effort call graph must not hang
/// a build.
fn trace(s: &Scorecard, from: &str) -> (Vec<Hop>, bool, Option<String>) {
    const MAX_DEPTH: usize = 6;
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut frontier = vec![from.to_string()];
    let mut hops = Vec::new();
    let mut through_port = false;
    let mut target = None;

    for depth in 0..MAX_DEPTH {
        let mut next = Vec::new();
        for id in &frontier {
            for e in s.model.edges.iter().filter(|e| &e.from == id) {
                if !matches!(e.kind, EdgeKind::Call | EdgeKind::Bound) {
                    continue;
                }
                let EdgeTarget::Local { id: to } = &e.to else { continue };
                let Some(sym) = s.model.symbol(to) else { continue };
                if !seen.insert(sym.id.as_str()) {
                    continue;
                }
                let owner = sym.parent.as_deref().unwrap_or(&sym.id);
                let pattern = s
                    .classified
                    .pattern_of(owner)
                    .or_else(|| s.classified.pattern_of(&sym.id))
                    .unwrap_or("unclassified");
                // A `Bound` edge names the trait itself, not one of its
                // methods; a `Call` to a free function has no owning type.
                // Repeating the name in both columns reads as a method
                // call on itself.
                let owner_name =
                    s.model.symbol(owner).map(|x| x.name.clone()).unwrap_or_default();
                let (cls, method) = if owner == sym.id {
                    (sym.module.clone(), sym.name.clone())
                } else {
                    (owner_name, sym.name.clone())
                };
                hops.push(Hop {
                    cls,
                    kind: pattern.to_string(),
                    method,
                    d: depth,
                    m: sym.module.clone(),
                });
                // The port is the terminus. Everything below it is reached
                // through the trait, not named by the caller.
                if pattern == "port" {
                    through_port = true;
                    // The first port reached is the terminus; a handler
                    // taking two ports should not report the second.
                    target.get_or_insert_with(|| owner.to_string());
                    continue;
                }
                next.push(sym.id.clone());
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    // Order by the flow, so the trace reads handler -> use-case -> port
    // rather than in edge-discovery order.
    hops.sort_by_key(|h| {
        (
            h.d,
            match h.kind.as_str() {
                "command-handler" => 0,
                "service" => 1,
                "use-case" => 2,
                "port" => 3,
                _ => 4,
            },
        )
    });
    (hops, through_port, target)
}

pub fn entry_count(s: &Scorecard) -> usize {
    entries(s).len()
}

pub fn build(s: &Scorecard) -> Traffic {
    let endpoints = entries(s);
    let diagnostics = endpoints
        .iter()
        .filter(|e| e.flat.is_empty())
        .map(|e| {
            format!(
                "{}:{} {} — no resolvable call out of this handler; \
                 dispatch is likely a match on a key code (spec 01: Call edges are best-effort)",
                e.file, e.line, e.route
            )
        })
        .collect();
    Traffic {
        generated_at: crate::baseline::now_iso(),
        // Unresolved traces are reported, not drawn.
        endpoints: endpoints.into_iter().filter(|e| !e.flat.is_empty()).collect(),
        diagnostics,
    }
}
