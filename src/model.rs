//! The symbol-level architecture model. See `specs/01-symbol-model.md`.
//!
//! Nothing here knows about `patterns-rust` or any particular project. The
//! scanner assigns no meaning; classification is spec 03.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SymbolKind {
    Module,
    Struct,
    Enum,
    Trait,
    Union,
    TypeAlias,
    Fn,
    Method,
    /// Deviation from spec 01, which lists eight kinds. `no-global-mut`
    /// (spec 04) needs to see `static CONFIG_OVERRIDE: Mutex<..>`, and a
    /// static is not reachable through any other kind.
    Static,
    Const,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Visibility {
    Private,
    Crate,
    Super,
    Public,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    /// The type as written, for display.
    pub ty: String,
    /// Every path mentioned in the type, including generic arguments.
    pub ty_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub ty: String,
    pub ty_paths: Vec<String>,
    /// `&impl Store`, `&dyn Store`, or a generic parameter with a bound.
    /// This is what separates "calls through the port" from naming a concrete.
    pub bounds: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FnSig {
    pub params: Vec<Param>,
    pub ret: Option<String>,
    pub ret_paths: Vec<String>,
    /// Bounds from generics and `where` clauses, e.g. `S: ActivityStore`.
    pub generic_bounds: Vec<String>,
    pub is_async: bool,
    /// Whether the fn takes a `self` receiver. Spec 01 discarded the
    /// receiver; spec 02 needs it, because an associated function is not a
    /// method and must not become an LCOM4 vertex — `App::new` would
    /// otherwise report as an isolated component in every type that has one.
    #[serde(default)]
    pub takes_self: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub id: String,
    pub kind: SymbolKind,
    pub name: String,
    pub module: String,
    /// Owning type, for methods.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    pub file: String,
    pub line: u32,
    pub vis: Visibility,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub cfg: Vec<String>,
    pub is_test: bool,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub derives: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub attrs: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub fields: Vec<Field>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sig: Option<FnSig>,
    /// `self.<field>` touched in a method body. Input to LCOM4 (spec 02).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub self_fields: Vec<String>,
    /// The subset of `self_fields` whose value is consumed. A field in
    /// `self_fields` and in no method's `self_reads` is write-only.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub self_reads: Vec<String>,
    /// Enum paths this body matches on. Input to the branching heuristics.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub matches_on: Vec<String>,
    /// Access chains two fields deep or more — reaching through one object
    /// to read another's state.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub field_chains: Vec<String>,
    /// `self.<method>()` called from a method body. Joins LCOM components.
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub self_calls: Vec<String>,
    pub loc: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "kebab-case")]
pub enum EdgeTarget {
    Local { id: String },
    Extern { krate: String, path: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EdgeKind {
    /// `use crate::x::Y` — module-level coupling.
    Use,
    /// `impl Trait for Type` — the port seam. Runs from the concrete to the
    /// trait, backwards against layer order. Exempt from flow rules (spec 03).
    Impl,
    /// A struct field whose type names another symbol.
    Field,
    /// A parameter or return type.
    Sig,
    /// `T: Store`, `&impl Store`, `&dyn Store` — calling through the port.
    Bound,
    /// A call in a function body. Best-effort.
    Call,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: EdgeTarget,
    pub kind: EdgeKind,
    pub file: String,
    pub line: u32,
    pub resolved: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticKind {
    UnresolvedPath,
    GlobImport,
    MacroItem,
    CfgExcluded,
    ParseFailure,
    MissingModule,
    /// A compilation unit cargo would build that this scan did not cover. A
    /// narrowed scan that does not say so is the failure this project names.
    TargetSkipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub file: String,
    pub line: u32,
    pub detail: String,
}

/// A trait/concrete binding, lifted out of `edges` for convenience.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImplBinding {
    /// Module the `impl` block sits in. Needed to resolve the trait name,
    /// which is written bare and expanded through that module's `use` map.
    pub module: String,
    pub trait_path: String,
    pub trait_id: Option<String>,
    pub type_path: String,
    pub type_id: Option<String>,
    pub file: String,
    pub line: u32,
    pub is_test: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleNode {
    pub id: String,
    pub file: String,
    pub parent: Option<String>,
    pub loc: u32,
    pub symbols: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Stats {
    pub files: usize,
    pub modules: usize,
    pub symbols: usize,
    pub types: usize,
    pub fns: usize,
    pub edges: usize,
    pub loc: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Model {
    pub project: String,
    pub root: String,
    pub generated_at: Option<String>,
    pub stats: Stats,
    pub modules: Vec<ModuleNode>,
    pub symbols: Vec<Symbol>,
    pub edges: Vec<Edge>,
    pub impls: Vec<ImplBinding>,
    /// Third-party and std paths named anywhere, with a count.
    pub externs: std::collections::BTreeMap<String, usize>,
    /// Every field name read through a base other than `self`, anywhere in
    /// the crate. A `pub` field absent from its own type's `self` accesses
    /// is not dead if some other module reads it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub foreign_field_reads: Vec<String>,
    /// Type ids whose `impl` blocks contain a macro invocation in item
    /// position. Their method set is incomplete, so spec 02 excludes them
    /// rather than proposing a decomposition built on half a type.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub macro_tainted: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Model {
    pub fn symbol(&self, id: &str) -> Option<&Symbol> {
        self.symbols.iter().find(|s| s.id == id)
    }

    /// Collapse the symbol graph to modules — the projection metatron's
    /// existing file-level views consume (spec 01, "Model output").
    pub fn file_links(&self) -> Vec<(String, String)> {
        let owner: std::collections::HashMap<&str, &str> = self
            .symbols
            .iter()
            .map(|s| (s.id.as_str(), s.module.as_str()))
            .collect();
        let mut out = std::collections::BTreeSet::new();
        for e in &self.edges {
            if let EdgeTarget::Local { id } = &e.to {
                let from = owner.get(e.from.as_str()).copied().unwrap_or_default();
                let to = owner.get(id.as_str()).copied().unwrap_or_default();
                if !from.is_empty() && !to.is_empty() && from != to {
                    out.insert((from.to_string(), to.to_string()));
                }
            }
        }
        out.into_iter().collect()
    }
}
