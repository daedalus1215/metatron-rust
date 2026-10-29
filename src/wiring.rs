//! The wiring model: bricks, sockets, studs. See `specs/11-workbench.md`.
//!
//! metatron-nestjs reads wiring off constructor injection: a constructor
//! names what it needs and a module decides what fills it. Rust has no
//! injector. A type is snapped into what its **fields** hold, and a function
//! into what its **parameters** take — `&impl ActivityStore` being the port
//! seam `patterns-rust` is built on. So:
//!
//! | NestJS | here |
//! |---|---|
//! | a class | a struct, enum or union, with every method of its `impl` blocks |
//! | a port (interface + token) | a trait: a hollow brick whose studs are its declared fns |
//! | a file of exported functions | a module's free functions, one brick per module (`module::*`) |
//! | a constructor parameter | a field, or a parameter of one of its fns |
//! | `this.x.method()` | `self.<field>.method()` and `<param>.method()` |
//! | `framework` | a type from another crate (`external`) |
//!
//! Calls on a receiver are resolved from what is **declared**, never
//! inferred: the field's or parameter's type, the type a `let` writes
//! (`let x: Foo`, `let x = Foo { .. }`), or the declared return type of the
//! fn a local came from (`let x = Foo::open()?`). A field whose type holds no
//! brick with that method is a library call and is passed over — the same
//! rule metatron-nestjs applies to `this.logger.log`. A local whose `let`
//! declares nothing readable is counted as unread, not guessed.
//!
//! Nothing here reads git, the network, or the clock. Rendering is
//! `views::workbench`; serving it is `serve`.

use crate::classify::{Classified, Config};
use crate::model::{DiagnosticKind, EdgeKind, EdgeTarget, Model, Symbol, SymbolKind, Visibility};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// Std and its facades are the language, not parts: a field holding a
/// `String` or a `HashMap` is data, and drawing it as a socket would bury
/// the ones that matter.
const LANGUAGE: &[&str] = &["std", "core", "alloc"];

#[derive(Serialize, Clone, Debug)]
pub struct Brick {
    pub id: String,
    pub file: String,
    pub name: String,
    /// `struct` | `enum` | `union` | `trait` | `functions`.
    pub shape: &'static str,
    pub module: String,
    pub pattern: String,
    pub tier: usize,
    /// 1-indexed, inclusive. For a type: its definition and every method in
    /// the same file. For a module's functions: the first to the last.
    pub lines: [u32; 2],
    pub sockets: Vec<Socket>,
    pub studs: Vec<Stud>,
    pub internals: Vec<Stud>,
    /// Always null: Rust has no inheritance. Kept because the workbench
    /// page reads it.
    pub extends: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Socket {
    /// The field or parameter name. Several that name the same target are
    /// one socket, listed together: `from, to`.
    pub prop: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub line: u32,
    /// `field` or `param`.
    pub via: &'static str,
    /// `resolved` (a type in the tree), `port` (a trait in the tree),
    /// `external` (a type from another crate), `unresolved`.
    pub status: &'static str,
    pub to: Option<String>,
    /// For a port with exactly one implementing brick outside tests: that
    /// brick. With several, the binding is the composition root's choice
    /// and this stays null rather than picking one.
    pub bound_to: Option<String>,
    /// For `external`: the path, as resolved (`rusqlite::Connection`).
    pub external: Option<String>,
    /// For `unresolved`: why.
    pub why: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Stud {
    pub name: String,
    pub sig: String,
    pub lines: [u32; 2],
    #[serde(rename = "async")]
    pub is_async: bool,
    /// `method` | `function` | `declared` (a trait's fn).
    pub kind: &'static str,
    #[serde(rename = "static")]
    pub is_static: bool,
    /// The entry id, when this fn is a CLI or TUI entry point.
    pub route: Option<String>,
    /// `brick` | `port` | `route` | `framework` | `unseen`. Internals: null.
    pub grip: Option<&'static str>,
    /// For `framework`: the trait from another crate it is called through.
    pub framework: Option<String>,
    /// The trait this fn implements, if any (`impl Trait for Type`).
    #[serde(rename = "trait")]
    pub of_trait: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Wire {
    pub from: String,
    pub to: String,
    pub prop: String,
    pub via: &'static str,
    pub bound_to: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub struct Call {
    pub from: String,
    pub from_method: Option<String>,
    pub to: String,
    pub to_method: Option<String>,
    pub line: u32,
    /// For a call through a port: the one brick implementing it, if there
    /// is exactly one.
    pub bound_to: Option<String>,
    /// `path` (`f()`, `Type::new()`), `field` (`self.store.get()`), `param`
    /// (`store.get()`), `local` (`x.get()` after `let x = Store::open()`) or
    /// `chain` (`Store::open().get()`).
    pub via: &'static str,
    #[serde(rename = "static")]
    pub is_static: bool,
}

/// `impl Trait for Type`, both bricks, outside tests. Drawn as the dotted
/// line from the port down to what fills it.
#[derive(Serialize, Clone, Debug)]
pub struct ImplLink {
    #[serde(rename = "trait")]
    pub of_trait: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub file: String,
    pub line: u32,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct StudCounts {
    pub total: usize,
    pub brick: usize,
    pub port: usize,
    pub route: usize,
    pub framework: usize,
    pub unseen: usize,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct RecvCounts {
    /// Followed to a brick through a declared field or parameter type.
    pub followed: usize,
    /// On a field or parameter whose type holds no brick with that method:
    /// a library call (`.clone()`, `.iter()`), passed over.
    pub library: usize,
    /// More than one brick behind the receiver has the method. Not drawn.
    pub ambiguous: usize,
    /// On a local, or a chained call, whose `let` or callee declares no type
    /// the scan can read. Not drawn: it may hide a call to a brick, or be a
    /// call on a `Vec`.
    pub unread: usize,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct WiringMeta {
    pub sockets: usize,
    /// Every socket the scan can say what fills: `resolved`, `port` and
    /// `external` together.
    pub resolved: usize,
    pub port: usize,
    pub external: usize,
    pub unresolved: usize,
    pub studs: StudCounts,
    pub recv_calls: RecvCounts,
}

#[derive(Serialize, Clone, Debug)]
pub struct WiringDiagnostic {
    pub kind: &'static str,
    pub file: String,
    pub line: u32,
    pub detail: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct Wiring {
    pub bricks: Vec<Brick>,
    pub wires: Vec<Wire>,
    pub calls: Vec<Call>,
    pub impls: Vec<ImplLink>,
    pub meta: WiringMeta,
    pub diagnostics: Vec<WiringDiagnostic>,
}

/// Build the wiring. `entries` maps an entry-point fn's symbol id to its
/// verb (`cli` | `tui`) — the traffic view's inbound signals, which grip a
/// stud from outside the code the way an HTTP route does.
pub fn build(
    model: &Model,
    config: &Config,
    classified: &Classified,
    entries: &BTreeMap<String, String>,
) -> Wiring {
    let sym: BTreeMap<&str, &Symbol> = model.symbols.iter().map(|s| (s.id.as_str(), s)).collect();
    let unclassified = config.layers.len();
    let tier_of = |layer: Option<&str>| {
        layer
            .and_then(|l| config.layers.iter().position(|x| x.id == l))
            .unwrap_or(unclassified)
    };

    // ------------------------------------------------------------ bricks
    // A brick per type and per trait, and one per module that has free fns.
    let mut owner: BTreeMap<&str, String> = BTreeMap::new(); // fn/method id -> brick id
    let mut bricks: BTreeMap<String, Brick> = BTreeMap::new();
    for s in &model.symbols {
        if s.is_test {
            continue;
        }
        let shape = match s.kind {
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Union => "union",
            SymbolKind::Trait => "trait",
            _ => continue,
        };
        bricks.insert(
            s.id.clone(),
            Brick {
                id: s.id.clone(),
                file: s.file.clone(),
                name: s.name.clone(),
                shape,
                module: module_label(&s.module),
                pattern: classified
                    .pattern_of(&s.id)
                    .unwrap_or("unclassified")
                    .to_string(),
                tier: tier_of(classified.layer_of(&s.id)),
                lines: span(s),
                sockets: vec![],
                studs: vec![],
                internals: vec![],
                extends: None,
            },
        );
    }
    // Each module's free fns vote on its brick's layer and pattern.
    let mut votes: BTreeMap<String, (Vec<&str>, Vec<&str>)> = BTreeMap::new();
    for s in &model.symbols {
        if s.is_test {
            continue;
        }
        match s.kind {
            SymbolKind::Method => {
                if let Some(p) = s.parent.as_deref().filter(|p| bricks.contains_key(*p)) {
                    owner.insert(&s.id, p.to_string());
                }
            }
            SymbolKind::Fn if s.parent.is_none() => {
                let id = fns_id(&s.module);
                owner.insert(&s.id, id.clone());
                let v = votes.entry(id.clone()).or_default();
                v.0.extend(classified.layer_of(&s.id));
                v.1.extend(classified.pattern_of(&s.id));
                bricks.entry(id.clone()).or_insert_with(|| Brick {
                    id,
                    file: s.file.clone(),
                    name: fns_name(&s.module, &s.file),
                    shape: "functions",
                    module: module_label(&s.module),
                    pattern: String::new(),
                    tier: unclassified,
                    lines: span(s),
                    sockets: vec![],
                    studs: vec![],
                    internals: vec![],
                    extends: None,
                });
            }
            _ => {}
        }
    }
    // A module's functions take the layer and pattern most of them have.
    // Splitting them one brick per layer would draw the module as several
    // parts that are one file; the city view is the lens for mixed layers.
    for (id, (layers, patterns)) in &votes {
        let b = bricks.get_mut(id).expect("inserted above");
        b.tier = tier_of(mode(layers.iter().copied()));
        b.pattern = mode(patterns.iter().copied())
            .unwrap_or("unclassified")
            .to_string();
    }

    // ---------------------------------------------------------- impls
    // The trait each method implements, and whether that trait is ours.
    let mut impl_of: BTreeMap<(String, String), (String, Option<String>)> = BTreeMap::new();
    let mut implementors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut impls = Vec::new();
    for b in &model.impls {
        if b.is_test {
            continue;
        }
        let Some(ty) = b.type_id.as_ref().filter(|t| bricks.contains_key(*t)) else {
            continue;
        };
        for m in &b.methods {
            impl_of.insert(
                (ty.clone(), m.clone()),
                (b.trait_path.clone(), b.trait_id.clone()),
            );
        }
        if let Some(tr) = b.trait_id.as_ref().filter(|t| bricks.contains_key(*t)) {
            implementors.entry(tr.clone()).or_default().push(ty.clone());
            impls.push(ImplLink {
                of_trait: tr.clone(),
                ty: ty.clone(),
                file: b.file.clone(),
                line: b.line,
            });
        }
    }
    for v in implementors.values_mut() {
        v.sort();
        v.dedup();
    }
    let bound = |trait_id: &str| -> Option<String> {
        match implementors.get(trait_id).map(|v| v.as_slice()) {
            Some([one]) => Some(one.clone()),
            _ => None,
        }
    };

    // ------------------------------------------------------------ lines
    // A type's lines run from its definition over every method in the same
    // file, so the source panel shows the methods it is built from.
    for (id, b) in &owner {
        let s = sym[id];
        if let Some(br) = bricks.get_mut(b) {
            if s.file == br.file {
                let [a, z] = span(s);
                br.lines = [br.lines[0].min(a), br.lines[1].max(z)];
            }
        }
    }

    // ------------------------------------------------------------ calls
    let is_trait = |id: &str| bricks.get(id).is_some_and(|b| b.shape == "trait");
    let mut calls: BTreeSet<Call> = BTreeSet::new();
    for e in &model.edges {
        if e.kind != EdgeKind::Call {
            continue;
        }
        let EdgeTarget::Local { id: to } = &e.to else {
            continue;
        };
        let Some(from) = owner.get(e.from.as_str()) else {
            continue;
        };
        let Some(t) = sym.get(to.as_str()) else {
            continue;
        };
        // A call lands on a fn or method (its brick, by name), or on a type
        // itself — a tuple struct or variant built in place.
        let (to_brick, to_method, is_static) = match t.kind {
            SymbolKind::Fn | SymbolKind::Method => match owner.get(t.id.as_str()) {
                Some(b) => (
                    b.clone(),
                    Some(t.name.clone()),
                    t.kind == SymbolKind::Method && !t.sig.as_ref().is_some_and(|g| g.takes_self),
                ),
                None => continue,
            },
            _ if bricks.contains_key(&t.id) => (t.id.clone(), None, true),
            _ => continue,
        };
        if &to_brick == from {
            continue;
        }
        calls.insert(Call {
            from: from.clone(),
            from_method: Some(sym[e.from.as_str()].name.clone()),
            bound_to: if is_trait(&to_brick) {
                bound(&to_brick)
            } else {
                None
            },
            to: to_brick,
            to_method,
            line: e.line,
            via: "path",
            is_static,
        });
    }

    // What a field or a parameter names, by (owner symbol, name).
    let mut named: BTreeMap<(&str, &str), Vec<&str>> = BTreeMap::new();
    for e in &model.edges {
        if !matches!(e.kind, EdgeKind::Field | EdgeKind::Sig | EdgeKind::Bound) {
            continue;
        }
        let (Some(via), EdgeTarget::Local { id }) = (e.via.as_deref(), &e.to) else {
            continue;
        };
        named
            .entry((e.from.as_str(), via))
            .or_default()
            .push(id.as_str());
    }
    // What each fn hands back: its return type's bricks, and its own type for
    // `-> Self`. A tuple struct or variant built by a call is its own type.
    let mut returns: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for e in &model.edges {
        if let (EdgeKind::Sig, Some("->"), EdgeTarget::Local { id }) =
            (e.kind, e.via.as_deref(), &e.to)
        {
            returns
                .entry(e.from.as_str())
                .or_default()
                .push(id.as_str());
        }
    }
    let returns_of = |callee: &str| -> Vec<&str> {
        let Some(c) = sym.get(callee) else {
            return vec![];
        };
        if bricks.contains_key(callee) {
            return vec![c.id.as_str()];
        }
        let mut v = returns.get(callee).cloned().unwrap_or_default();
        let says_self = c
            .sig
            .as_ref()
            .and_then(|g| g.ret.as_deref())
            .is_some_and(|r| {
                r.split(|ch: char| !ch.is_alphanumeric() && ch != '_')
                    .any(|w| w == "Self")
            });
        if let (true, Some(p)) = (says_self, c.parent.as_deref()) {
            v.push(p);
        }
        v
    };
    let has_method = |brick: &str, m: &str| {
        let id = format!("{brick}::{m}");
        sym.get(id.as_str())
            .is_some_and(|s| owner.get(s.id.as_str()).is_some_and(|b| b == brick))
    };
    let mut recv = RecvCounts::default();
    for s in &model.symbols {
        let Some(from) = owner.get(s.id.as_str()) else {
            continue;
        };
        for rc in &s.recv_calls {
            let is_param = || {
                s.sig
                    .as_ref()
                    .is_some_and(|g| g.params.iter().any(|p| p.name == rc.recv))
            };
            // What the receiver is declared to be, nearest declaration first:
            // a field, then a `let` (which shadows a parameter), then the
            // parameter. A local whose `let` declares nothing is not guessed.
            let (targets, via): (Vec<&str>, &'static str) =
                if let Some(f) = rc.recv.strip_prefix("self.") {
                    match s.parent.as_deref() {
                        Some(p) => (named.get(&(p, f)).cloned().unwrap_or_default(), "field"),
                        None => continue,
                    }
                } else if let Some(t) = rc.ty.as_deref() {
                    (vec![t], "local")
                } else if let Some(c) = rc.callee.as_deref() {
                    (
                        returns_of(c),
                        if rc.recv == "()" { "chain" } else { "local" },
                    )
                } else if is_param() {
                    (
                        named
                            .get(&(s.id.as_str(), rc.recv.as_str()))
                            .cloned()
                            .unwrap_or_default(),
                        "param",
                    )
                } else {
                    recv.unread += 1;
                    continue;
                };
            if targets.is_empty() {
                recv.library += 1;
                continue;
            }
            let mut hit: Vec<&str> = targets
                .iter()
                .copied()
                .filter(|t| bricks.contains_key(*t) && has_method(t, &rc.method))
                .collect();
            hit.sort();
            hit.dedup();
            let to = match hit.as_slice() {
                [] => {
                    recv.library += 1;
                    continue;
                }
                [one] => one.to_string(),
                _ => {
                    recv.ambiguous += 1;
                    continue;
                }
            };
            recv.followed += 1;
            if &to == from {
                continue;
            }
            calls.insert(Call {
                from: from.clone(),
                from_method: Some(s.name.clone()),
                bound_to: if is_trait(&to) { bound(&to) } else { None },
                to,
                to_method: Some(rc.method.clone()),
                line: rc.line,
                via,
                is_static: false,
            });
        }
    }
    let calls: Vec<Call> = calls.into_iter().collect();

    // --------------------------------------------------------- sockets
    let mut diagnostics = Vec::new();
    let mut sockets: BTreeMap<String, Vec<Socket>> = BTreeMap::new();
    let put = |sockets: &mut BTreeMap<String, Vec<Socket>>, brick: &str, s: Socket| {
        let list = sockets.entry(brick.to_string()).or_default();
        let key = |x: &Socket| (x.status, x.to.clone(), x.external.clone());
        match list
            .iter_mut()
            .find(|x| x.status != "unresolved" && key(x) == key(&s))
        {
            Some(x) => {
                if !x.prop.split(", ").any(|p| p == s.prop) {
                    x.prop = format!("{}, {}", x.prop, s.prop);
                }
                if x.via != s.via {
                    x.via = "field";
                }
            }
            None => list.push(s),
        }
    };
    // Fields first, in declaration order, then the parameters of each fn.
    let mut owners: Vec<(&Symbol, &str)> = model
        .symbols
        .iter()
        .filter(|s| !s.is_test && bricks.contains_key(&s.id) && !s.fields.is_empty())
        .map(|s| (s, s.id.as_str()))
        .collect();
    owners.extend(
        model
            .symbols
            .iter()
            .filter_map(|s| owner.get(s.id.as_str()).map(|b| (s, b.as_str()))),
    );
    for (s, brick) in &owners {
        let names: Vec<(&str, &str, u32, &'static str)> =
            if s.kind == SymbolKind::Fn || s.kind == SymbolKind::Method {
                s.sig
                    .iter()
                    .flat_map(|g| {
                        g.params
                            .iter()
                            .map(|p| (p.name.as_str(), p.ty.as_str(), s.line, "param"))
                    })
                    .collect()
            } else {
                s.fields
                    .iter()
                    .map(|f| (f.name.as_str(), f.ty.as_str(), s.line, "field"))
                    .collect()
            };
        for (name, ty, line, via) in names {
            for e in model.edges.iter().filter(|e| {
                e.from == s.id
                    && e.via.as_deref() == Some(name)
                    && matches!(e.kind, EdgeKind::Field | EdgeKind::Sig | EdgeKind::Bound)
            }) {
                let sock = match &e.to {
                    EdgeTarget::Local { id } => match bricks.get(id) {
                        Some(t) if t.shape == "trait" => Socket {
                            prop: name.into(),
                            ty: ty.into(),
                            line,
                            via,
                            status: "port",
                            to: Some(id.clone()),
                            bound_to: bound(id),
                            external: None,
                            why: None,
                        },
                        Some(_) => Socket {
                            prop: name.into(),
                            ty: ty.into(),
                            line,
                            via,
                            status: "resolved",
                            to: Some(id.clone()),
                            bound_to: None,
                            external: None,
                            why: None,
                        },
                        // An alias, a const, a test type: in the tree, not a part.
                        None => continue,
                    },
                    EdgeTarget::Extern { krate, path } => {
                        if LANGUAGE.contains(&krate.as_str()) {
                            continue;
                        }
                        Socket {
                            prop: name.into(),
                            ty: ty.into(),
                            line,
                            via,
                            status: "external",
                            to: None,
                            bound_to: None,
                            external: Some(path.clone()),
                            why: None,
                        }
                    }
                };
                if sock.to.as_deref() == Some(*brick) {
                    // A type naming itself (`next: Option<Box<Node>>`), or a
                    // fn of a type taking `&Self`'s own type: not a socket.
                    continue;
                }
                put(&mut sockets, brick, sock);
            }
        }
    }
    // A path the scan could not place leaves no edge, only a diagnostic on
    // the owner's line. Attribute it to the field or parameter that names it,
    // so a socket nobody can fill is counted rather than drawn as absent.
    let mut at_line: BTreeMap<(&str, u32), Vec<(&Symbol, &str)>> = BTreeMap::new();
    for (s, b) in &owners {
        at_line
            .entry((s.file.as_str(), s.line))
            .or_default()
            .push((s, b));
    }
    for d in &model.diagnostics {
        if d.kind != DiagnosticKind::UnresolvedPath {
            continue;
        }
        let Some(path) = d.detail.split('`').nth(1) else {
            continue;
        };
        let leaf = path.rsplit("::").next().unwrap_or(path);
        for (s, brick) in at_line
            .get(&(d.file.as_str(), d.line))
            .into_iter()
            .flatten()
        {
            let generics: BTreeSet<&str> = s
                .sig
                .iter()
                .flat_map(|g| g.generics.iter().map(|x| x.name.as_str()))
                .collect();
            if generics.contains(path) {
                continue;
            }
            let names: Vec<(&str, &str, &Vec<String>, &'static str)> = match &s.sig {
                Some(g) => g
                    .params
                    .iter()
                    .map(|p| (p.name.as_str(), p.ty.as_str(), &p.ty_paths, "param"))
                    .collect(),
                None => s
                    .fields
                    .iter()
                    .map(|f| (f.name.as_str(), f.ty.as_str(), &f.ty_paths, "field"))
                    .collect(),
            };
            for (name, ty, paths, via) in names {
                if !paths.iter().any(|p| p.rsplit("::").next() == Some(leaf)) {
                    continue;
                }
                diagnostics.push(WiringDiagnostic {
                    kind: "socket-unresolved",
                    file: d.file.clone(),
                    line: d.line,
                    detail: format!("{} `{name}: {ty}` — {}", s.name, d.detail),
                });
                put(
                    &mut sockets,
                    brick,
                    Socket {
                        prop: name.into(),
                        ty: ty.into(),
                        line: s.line,
                        via,
                        status: "unresolved",
                        to: None,
                        bound_to: None,
                        external: None,
                        why: Some(d.detail.clone()),
                    },
                );
            }
        }
    }

    // ----------------------------------------------------------- studs
    let mut gripped: BTreeSet<(&str, &str)> = BTreeSet::new();
    for c in &calls {
        if let Some(m) = &c.to_method {
            gripped.insert((c.to.as_str(), m.as_str()));
        }
    }
    let mut parts: BTreeMap<String, (Vec<Stud>, Vec<Stud>)> = BTreeMap::new();
    for (id, brick) in &owner {
        let s = sym[id];
        let g = s.sig.clone().unwrap_or_default();
        let from_trait = impl_of.get(&(brick.clone(), s.name.clone()));
        let in_trait = bricks[brick].shape == "trait";
        let public = in_trait || from_trait.is_some() || s.vis != Visibility::Private;
        let route = entries.get(&s.id).map(|_| s.id.clone());
        let grip = if !public {
            None
        } else if gripped.contains(&(brick.as_str(), s.name.as_str())) {
            Some("brick")
        } else if route.is_some() {
            Some("route")
        } else if let Some((_, Some(tr))) = from_trait {
            // Reached through the port: a brick calls the trait's fn.
            if gripped.contains(&(tr.as_str(), s.name.as_str())) {
                Some("port")
            } else {
                Some("unseen")
            }
        } else if from_trait.is_some() {
            Some("framework")
        } else {
            Some("unseen")
        };
        let stud = Stud {
            name: s.name.clone(),
            sig: sig_string(s),
            lines: span(s),
            is_async: g.is_async,
            kind: if in_trait {
                "declared"
            } else if s.kind == SymbolKind::Fn {
                "function"
            } else {
                "method"
            },
            is_static: s.kind == SymbolKind::Method && !g.takes_self,
            route,
            grip,
            framework: match (grip, from_trait) {
                (Some("framework"), Some((p, _))) => Some(format!("impl {p}")),
                _ => None,
            },
            of_trait: from_trait.map(|(p, _)| p.clone()),
        };
        let e = parts.entry(brick.clone()).or_default();
        if public {
            e.0.push(stud);
        } else {
            e.1.push(stud);
        }
    }

    // --------------------------------------------------------- assemble
    let mut meta = WiringMeta {
        recv_calls: recv,
        ..Default::default()
    };
    let mut wires = Vec::new();
    for (id, b) in bricks.iter_mut() {
        if let Some((mut st, mut int)) = parts.remove(id) {
            st.sort_by_key(|x| x.lines[0]);
            int.sort_by_key(|x| x.lines[0]);
            b.studs = st;
            b.internals = int;
        }
        b.sockets = sockets.remove(id).unwrap_or_default();
        for s in &b.sockets {
            meta.sockets += 1;
            match s.status {
                "port" => meta.port += 1,
                "external" => meta.external += 1,
                "unresolved" => meta.unresolved += 1,
                _ => {}
            }
            if let (Some(to), "port" | "resolved") = (&s.to, s.status) {
                wires.push(Wire {
                    from: id.clone(),
                    to: to.clone(),
                    prop: s.prop.clone(),
                    via: s.via,
                    bound_to: s.bound_to.clone(),
                });
            }
        }
        for s in &b.studs {
            meta.studs.total += 1;
            match s.grip {
                Some("brick") => meta.studs.brick += 1,
                Some("port") => meta.studs.port += 1,
                Some("route") => meta.studs.route += 1,
                Some("framework") => meta.studs.framework += 1,
                _ => meta.studs.unseen += 1,
            }
        }
    }
    meta.resolved = meta.sockets - meta.unresolved;

    Wiring {
        bricks: bricks.into_values().collect(),
        wires,
        calls,
        impls,
        meta,
        diagnostics,
    }
}

/// The id of a module's free-functions brick: the module, then `::*`.
fn fns_id(module: &str) -> String {
    format!("{}::*", module_label(module))
}

fn module_label(module: &str) -> String {
    if module.is_empty() {
        "(crate)".into()
    } else {
        module.into()
    }
}

/// `activity::*` for a module, or the file's own name for a crate root,
/// whose module id is synthetic: `main.rs`, not `(bin:metatron)::*`.
fn fns_name(module: &str, file: &str) -> String {
    let last = module.rsplit("::").next().unwrap_or(module);
    if last.is_empty() || last.starts_with('(') {
        file.rsplit('/').next().unwrap_or(file).to_string()
    } else {
        format!("{last}::*")
    }
}

/// The most frequent value; a tie goes to the first in sort order, so the
/// answer does not depend on the order symbols were scanned in.
fn mode<'a>(values: impl Iterator<Item = &'a str>) -> Option<&'a str> {
    let mut n: BTreeMap<&str, usize> = BTreeMap::new();
    for v in values {
        *n.entry(v).or_default() += 1;
    }
    n.into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(a.0)))
        .map(|(k, _)| k)
}

fn span(s: &Symbol) -> [u32; 2] {
    [s.line, s.line + s.loc.max(1) - 1]
}

/// `fn open_session(&self, id: i64, now: i64)`, as near to written as the
/// model keeps it.
fn sig_string(s: &Symbol) -> String {
    let Some(g) = &s.sig else {
        return s.name.clone();
    };
    let mut args: Vec<String> = g.receiver.iter().cloned().collect();
    args.extend(g.params.iter().map(|p| format!("{}: {}", p.name, p.ty)));
    format!(
        "{}fn {}({}){}",
        if g.is_async { "async " } else { "" },
        s.name,
        args.join(", "),
        g.ret
            .as_ref()
            .map(|r| format!(" -> {r}"))
            .unwrap_or_default()
    )
}
