//! The `syn` scanner. See `specs/01-symbol-model.md`.
//!
//! Three passes: discover the module tree by following `mod` declarations,
//! extract items into symbols and raw (unresolved) edges, then resolve every
//! path against a crate-wide symbol table. Anything still unresolved becomes a
//! diagnostic — never a guess.

pub mod body;
pub mod types;

use crate::model::*;
use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use syn::spanned::Spanned;
use types::{bounds_in_type, generic_bounds, path_string, paths_in_type, type_to_string};

const BUILTIN_CRATES: &[&str] = &["std", "core", "alloc", "proc_macro"];

struct RawEdge {
    from: String,
    raw: String,
    kind: EdgeKind,
    file: String,
    line: u32,
    module: String,
}

#[derive(Default)]
struct ModuleCtx {
    /// local name -> full crate-relative path, from `use` statements
    uses: HashMap<String, String>,
}

pub struct Scanner {
    root: PathBuf,
    project: String,
    extern_crates: BTreeSet<String>,
    symbols: Vec<Symbol>,
    raw_edges: Vec<RawEdge>,
    diagnostics: Vec<Diagnostic>,
    modules: Vec<ModuleNode>,
    impls: Vec<ImplBinding>,
    macro_tainted: BTreeSet<String>,
    foreign_field_reads: BTreeSet<String>,
    mod_ctx: HashMap<String, ModuleCtx>,
    files: usize,
    total_loc: u32,
}

pub fn scan(dir: &Path) -> Result<Model> {
    Scanner::new(dir)?.run()
}

impl Scanner {
    pub fn new(dir: &Path) -> Result<Self> {
        let dir = dir
            .canonicalize()
            .with_context(|| format!("no such directory: {}", dir.display()))?;
        let manifest = dir.join("Cargo.toml");
        let (project, extern_crates) = read_manifest(&manifest)?;
        Ok(Self {
            root: dir.join("src"),
            project,
            extern_crates,
            symbols: Vec::new(),
            raw_edges: Vec::new(),
            diagnostics: Vec::new(),
            modules: Vec::new(),
            impls: Vec::new(),
            macro_tainted: BTreeSet::new(),
            foreign_field_reads: BTreeSet::new(),
            mod_ctx: HashMap::new(),
            files: 0,
            total_loc: 0,
        })
    }

    pub fn run(mut self) -> Result<Model> {
        let entry = ["main.rs", "lib.rs"]
            .iter()
            .map(|f| self.root.join(f))
            .find(|p| p.exists())
            .with_context(|| format!("no main.rs or lib.rs under {}", self.root.display()))?;

        self.walk_module("", &entry, true)?;

        let (edges, externs) = self.resolve();

        let types = self
            .symbols
            .iter()
            .filter(|s| {
                matches!(
                    s.kind,
                    SymbolKind::Struct | SymbolKind::Enum | SymbolKind::Trait | SymbolKind::Union
                )
            })
            .count();
        let fns = self
            .symbols
            .iter()
            .filter(|s| matches!(s.kind, SymbolKind::Fn | SymbolKind::Method))
            .count();

        Ok(Model {
            project: self.project,
            root: self.root.display().to_string(),
            generated_at: None,
            stats: Stats {
                files: self.files,
                modules: self.modules.len(),
                symbols: self.symbols.len(),
                types,
                fns,
                edges: edges.len(),
                loc: self.total_loc,
            },
            modules: self.modules,
            symbols: self.symbols,
            edges,
            impls: self.impls,
            externs,
            macro_tainted: self.macro_tainted.into_iter().collect(),
            foreign_field_reads: self.foreign_field_reads.into_iter().collect(),
            diagnostics: self.diagnostics,
        })
    }

    // ---------------------------------------------------------- module tree

    fn walk_module(&mut self, mod_id: &str, file: &Path, is_root: bool) -> Result<()> {
        let src = std::fs::read_to_string(file)
            .with_context(|| format!("cannot read {}", file.display()))?;
        let rel = self.rel(file);
        let ast = match syn::parse_file(&src) {
            Ok(a) => a,
            Err(e) => {
                self.diagnostics.push(Diagnostic {
                    kind: DiagnosticKind::ParseFailure,
                    file: rel.clone(),
                    line: e.span().start().line as u32,
                    detail: e.to_string(),
                });
                return Ok(());
            }
        };

        self.files += 1;
        let loc = src.lines().count() as u32;
        self.total_loc += loc;
        let before = self.symbols.len();

        self.modules.push(ModuleNode {
            id: if mod_id.is_empty() {
                "(crate)".into()
            } else {
                mod_id.into()
            },
            file: rel.clone(),
            parent: parent_of(mod_id),
            loc,
            symbols: 0,
        });
        let mod_index = self.modules.len() - 1;

        self.items(&ast.items, mod_id, &rel, file, is_root, false)?;

        let added = self.symbols.len() - before;
        self.modules[mod_index].symbols = added;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn items(
        &mut self,
        items: &[syn::Item],
        mod_id: &str,
        rel: &str,
        file: &Path,
        is_root: bool,
        in_test: bool,
    ) -> Result<()> {
        for item in items {
            match item {
                syn::Item::Mod(m) => {
                    let name = m.ident.to_string();
                    let child = join(mod_id, &name);
                    let test = in_test || has_cfg_test(&m.attrs);
                    match &m.content {
                        Some((_, inner)) => {
                            self.items(inner, &child, rel, file, is_root, test)?;
                        }
                        None => {
                            match self.module_file(file, mod_id, &name, is_root, &m.attrs) {
                                Some(path) => self.walk_module(&child, &path, false)?,
                                None => self.diagnostics.push(Diagnostic {
                                    kind: DiagnosticKind::MissingModule,
                                    file: rel.into(),
                                    line: m.span().start().line as u32,
                                    detail: format!("`mod {name};` has no matching file"),
                                }),
                            }
                        }
                    }
                }
                syn::Item::Use(u) => self.use_item(u, mod_id, rel, in_test),
                syn::Item::Struct(s) => {
                    let fields = fields_of(&s.fields);
                    self.push(Symbol {
                        id: join(mod_id, &s.ident.to_string()),
                        kind: SymbolKind::Struct,
                        name: s.ident.to_string(),
                        module: mod_id.into(),
                        parent: None,
                        file: rel.into(),
                        line: s.span().start().line as u32,
                        vis: vis_of(&s.vis),
                        cfg: cfgs(&s.attrs),
                        is_test: in_test || has_cfg_test(&s.attrs),
                        derives: derives(&s.attrs),
                        attrs: attr_names(&s.attrs),
                        fields: fields.clone(),
                        sig: None,
                        self_reads: vec![],
                        matches_on: vec![],
                        field_chains: vec![],
                        self_fields: vec![],
                        self_calls: vec![],
                        loc: span_loc(s.span()),
                    });
                    let id = join(mod_id, &s.ident.to_string());
                    for f in &fields {
                        for p in &f.ty_paths {
                            self.edge(&id, p, EdgeKind::Field, rel, s.span(), mod_id);
                        }
                    }
                }
                syn::Item::Enum(e) => {
                    let id = join(mod_id, &e.ident.to_string());
                    let mut fields = Vec::new();
                    for v in &e.variants {
                        for f in fields_of(&v.fields) {
                            fields.push(Field {
                                name: format!("{}::{}", v.ident, f.name),
                                ..f
                            });
                        }
                    }
                    for f in &fields {
                        for p in &f.ty_paths {
                            self.edge(&id, p, EdgeKind::Field, rel, e.span(), mod_id);
                        }
                    }
                    self.push(Symbol {
                        id,
                        kind: SymbolKind::Enum,
                        name: e.ident.to_string(),
                        module: mod_id.into(),
                        parent: None,
                        file: rel.into(),
                        line: e.span().start().line as u32,
                        vis: vis_of(&e.vis),
                        cfg: cfgs(&e.attrs),
                        is_test: in_test || has_cfg_test(&e.attrs),
                        derives: derives(&e.attrs),
                        attrs: attr_names(&e.attrs),
                        fields,
                        sig: None,
                        self_reads: vec![],
                        matches_on: vec![],
                        field_chains: vec![],
                        self_fields: vec![],
                        self_calls: vec![],
                        loc: span_loc(e.span()),
                    });
                }
                syn::Item::Trait(t) => {
                    let id = join(mod_id, &t.ident.to_string());
                    self.push(Symbol {
                        id: id.clone(),
                        kind: SymbolKind::Trait,
                        name: t.ident.to_string(),
                        module: mod_id.into(),
                        parent: None,
                        file: rel.into(),
                        line: t.span().start().line as u32,
                        vis: vis_of(&t.vis),
                        cfg: cfgs(&t.attrs),
                        is_test: in_test || has_cfg_test(&t.attrs),
                        derives: vec![],
                        attrs: attr_names(&t.attrs),
                        fields: vec![],
                        sig: None,
                        self_reads: vec![],
                        matches_on: vec![],
                        field_chains: vec![],
                        self_fields: vec![],
                        self_calls: vec![],
                        loc: span_loc(t.span()),
                    });
                    for ti in &t.items {
                        if let syn::TraitItem::Fn(f) = ti {
                            let sig = sig_of(&f.sig);
                            let mid = format!("{id}::{}", f.sig.ident);
                            self.sig_edges(&mid, &sig, rel, f.span(), mod_id);
                            self.push(Symbol {
                                id: mid,
                                kind: SymbolKind::Method,
                                name: f.sig.ident.to_string(),
                                module: mod_id.into(),
                                parent: Some(id.clone()),
                                file: rel.into(),
                                line: f.span().start().line as u32,
                                vis: Visibility::Public,
                                cfg: cfgs(&f.attrs),
                                is_test: in_test,
                                derives: vec![],
                                attrs: attr_names(&f.attrs),
                                fields: vec![],
                                sig: Some(sig),
                                self_reads: vec![],
                                matches_on: vec![],
                                field_chains: vec![],
                                self_fields: vec![],
                                self_calls: vec![],
                                loc: span_loc(f.span()),
                            });
                        }
                    }
                }
                syn::Item::Union(u) => {
                    self.push(Symbol {
                        id: join(mod_id, &u.ident.to_string()),
                        kind: SymbolKind::Union,
                        name: u.ident.to_string(),
                        module: mod_id.into(),
                        parent: None,
                        file: rel.into(),
                        line: u.span().start().line as u32,
                        vis: vis_of(&u.vis),
                        cfg: cfgs(&u.attrs),
                        is_test: in_test || has_cfg_test(&u.attrs),
                        derives: derives(&u.attrs),
                        attrs: attr_names(&u.attrs),
                        fields: fields_of(&syn::Fields::Named(u.fields.clone())),
                        sig: None,
                        self_reads: vec![],
                        matches_on: vec![],
                        field_chains: vec![],
                        self_fields: vec![],
                        self_calls: vec![],
                        loc: span_loc(u.span()),
                    });
                }
                syn::Item::Type(t) => self.push(Symbol {
                    id: join(mod_id, &t.ident.to_string()),
                    kind: SymbolKind::TypeAlias,
                    name: t.ident.to_string(),
                    module: mod_id.into(),
                    parent: None,
                    file: rel.into(),
                    line: t.span().start().line as u32,
                    vis: vis_of(&t.vis),
                    cfg: cfgs(&t.attrs),
                    is_test: in_test,
                    derives: vec![],
                    attrs: attr_names(&t.attrs),
                    fields: vec![],
                    sig: None,
                    self_reads: vec![],
                    matches_on: vec![],
                    field_chains: vec![],
                    self_fields: vec![],
                    self_calls: vec![],
                    loc: span_loc(t.span()),
                }),
                syn::Item::Fn(f) => {
                    let id = join(mod_id, &f.sig.ident.to_string());
                    let sig = sig_of(&f.sig);
                    let scan = body::BodyScan::of(&f.block);
                    self.foreign_field_reads
                        .extend(scan.foreign_fields.iter().cloned());
                    self.sig_edges(&id, &sig, rel, f.span(), mod_id);
                    for (c, line) in &scan.calls {
                        self.raw_edges.push(RawEdge {
                            from: id.clone(),
                            raw: c.clone(),
                            kind: EdgeKind::Call,
                            file: rel.into(),
                            line: *line,
                            module: mod_id.into(),
                        });
                    }
                    self.push(Symbol {
                        id,
                        kind: SymbolKind::Fn,
                        name: f.sig.ident.to_string(),
                        module: mod_id.into(),
                        parent: None,
                        file: rel.into(),
                        line: f.span().start().line as u32,
                        vis: vis_of(&f.vis),
                        cfg: cfgs(&f.attrs),
                        is_test: in_test || has_cfg_test(&f.attrs) || is_test_fn(&f.attrs),
                        derives: vec![],
                        attrs: attr_names(&f.attrs),
                        fields: vec![],
                        sig: Some(sig),
                        // A free function has no `self`, so spec 01 left
                        // these empty. `matches_on` and `field_chains` are
                        // meaningful for one, and spec 04's branching
                        // heuristics read them.
                        self_reads: vec![],
                        matches_on: scan.matches_on,
                        field_chains: scan.field_chains,
                        self_fields: vec![],
                        self_calls: vec![],
                        loc: span_loc(f.span()),
                    });
                }
                syn::Item::Impl(i) => self.impl_item(i, mod_id, rel, in_test),
                syn::Item::Static(s) => self.push(Symbol {
                    id: join(mod_id, &s.ident.to_string()),
                    kind: SymbolKind::Static,
                    name: s.ident.to_string(),
                    module: mod_id.into(),
                    parent: None,
                    file: rel.into(),
                    line: s.span().start().line as u32,
                    vis: vis_of(&s.vis),
                    cfg: cfgs(&s.attrs),
                    is_test: in_test,
                    derives: vec![],
                    attrs: attr_names(&s.attrs),
                    fields: vec![Field {
                        name: s.ident.to_string(),
                        ty: match s.mutability {
                            syn::StaticMutability::Mut(_) => {
                                format!("mut {}", type_to_string(&s.ty))
                            }
                            _ => type_to_string(&s.ty),
                        },
                        ty_paths: paths_in_type(&s.ty),
                    }],
                    sig: None,
                    self_reads: vec![],
                    matches_on: vec![],
                    field_chains: vec![],
                    self_fields: vec![],
                    self_calls: vec![],
                    loc: span_loc(s.span()),
                }),
                syn::Item::Const(c) => self.push(Symbol {
                    id: join(mod_id, &c.ident.to_string()),
                    kind: SymbolKind::Const,
                    name: c.ident.to_string(),
                    module: mod_id.into(),
                    parent: None,
                    file: rel.into(),
                    line: c.span().start().line as u32,
                    vis: vis_of(&c.vis),
                    cfg: cfgs(&c.attrs),
                    is_test: in_test,
                    derives: vec![],
                    attrs: attr_names(&c.attrs),
                    fields: vec![],
                    sig: None,
                    self_reads: vec![],
                    matches_on: vec![],
                    field_chains: vec![],
                    self_fields: vec![],
                    self_calls: vec![],
                    loc: span_loc(c.span()),
                }),
                syn::Item::Macro(m) => {
                    // `macro_rules!` and any item-position macro invocation.
                    // syn does not expand; whatever it produces is invisible.
                    self.diagnostics.push(Diagnostic {
                        kind: DiagnosticKind::MacroItem,
                        file: rel.into(),
                        line: m.span().start().line as u32,
                        detail: format!(
                            "item-position macro `{}!` — generated items are not modelled",
                            path_string(&m.mac.path)
                        ),
                    });
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn impl_item(&mut self, i: &syn::ItemImpl, mod_id: &str, rel: &str, in_test: bool) {
        let self_paths = paths_in_type(&i.self_ty);
        let self_name = self_paths.first().cloned().unwrap_or_default();
        let type_id = join(mod_id, last_seg(&self_name));
        let is_test = in_test || has_cfg_test(&i.attrs);

        if let Some((_, trait_path, _)) = &i.trait_ {
            let tp = path_string(trait_path);
            // The port seam. From the concrete to the trait — backwards
            // against layer order, and the whole point of the architecture.
            self.raw_edges.push(RawEdge {
                from: type_id.clone(),
                raw: tp.clone(),
                kind: EdgeKind::Impl,
                file: rel.into(),
                line: i.span().start().line as u32,
                module: mod_id.into(),
            });
            self.impls.push(ImplBinding {
                module: mod_id.into(),
                trait_path: tp,
                trait_id: None,
                type_path: self_name.clone(),
                type_id: Some(type_id.clone()),
                file: rel.into(),
                line: i.span().start().line as u32,
                is_test,
            });
        }

        for item in &i.items {
            if let syn::ImplItem::Macro(mac) = item {
                // The method set is now incomplete. Spec 02 excludes the type
                // rather than proposing a split built on half of it.
                self.macro_tainted.insert(type_id.clone());
                self.diagnostics.push(Diagnostic {
                    kind: DiagnosticKind::MacroItem,
                    file: rel.into(),
                    line: mac.span().start().line as u32,
                    detail: format!(
                        "`{}!` inside `impl {}` — generated methods are not modelled",
                        path_string(&mac.mac.path),
                        self_name
                    ),
                });
            }
            if let syn::ImplItem::Fn(f) = item {
                let mid = format!("{type_id}::{}", f.sig.ident);
                let sig = sig_of(&f.sig);
                let scan = body::BodyScan::of(&f.block);
                self.foreign_field_reads
                    .extend(scan.foreign_fields.iter().cloned());
                self.sig_edges(&mid, &sig, rel, f.span(), mod_id);
                for (c, line) in &scan.calls {
                    self.raw_edges.push(RawEdge {
                        from: mid.clone(),
                        raw: c.clone(),
                        kind: EdgeKind::Call,
                        file: rel.into(),
                        line: *line,
                        module: mod_id.into(),
                    });
                }
                self.push(Symbol {
                    id: mid,
                    kind: SymbolKind::Method,
                    name: f.sig.ident.to_string(),
                    module: mod_id.into(),
                    parent: Some(type_id.clone()),
                    file: rel.into(),
                    line: f.span().start().line as u32,
                    vis: vis_of(&f.vis),
                    cfg: cfgs(&f.attrs),
                    is_test: is_test || is_test_fn(&f.attrs),
                    derives: vec![],
                    attrs: attr_names(&f.attrs),
                    fields: vec![],
                    sig: Some(sig),
                    self_reads: scan.self_reads,
                    matches_on: scan.matches_on,
                    field_chains: scan.field_chains,
                    self_fields: scan.self_fields,
                    self_calls: scan.self_calls,
                    loc: span_loc(f.span()),
                });
            }
        }
    }

    fn use_item(&mut self, u: &syn::ItemUse, mod_id: &str, rel: &str, _in_test: bool) {
        let line = u.span().start().line as u32;
        let mut leaves = Vec::new();
        flatten_use(&u.tree, String::new(), &mut leaves, &mut |glob| {
            self.diagnostics.push(Diagnostic {
                kind: DiagnosticKind::GlobImport,
                file: rel.into(),
                line,
                detail: format!("`use {glob}::*` — members resolved at low confidence"),
            });
        });
        let ctx = self.mod_ctx.entry(mod_id.to_string()).or_default();
        for (path, alias) in &leaves {
            ctx.uses.insert(alias.clone(), path.clone());
        }
        for (path, _) in leaves {
            self.raw_edges.push(RawEdge {
                from: if mod_id.is_empty() {
                    "(crate)".into()
                } else {
                    mod_id.into()
                },
                raw: path,
                kind: EdgeKind::Use,
                file: rel.into(),
                line,
                module: mod_id.into(),
            });
        }
    }

    fn sig_edges(
        &mut self,
        from: &str,
        sig: &FnSig,
        rel: &str,
        span: proc_macro2::Span,
        mod_id: &str,
    ) {
        for p in &sig.params {
            for b in &p.bounds {
                self.edge(from, b, EdgeKind::Bound, rel, span, mod_id);
            }
            for t in &p.ty_paths {
                if !p.bounds.contains(t) {
                    self.edge(from, t, EdgeKind::Sig, rel, span, mod_id);
                }
            }
        }
        for t in &sig.ret_paths {
            self.edge(from, t, EdgeKind::Sig, rel, span, mod_id);
        }
        for b in &sig.generic_bounds {
            self.edge(from, b, EdgeKind::Bound, rel, span, mod_id);
        }
    }

    fn edge(
        &mut self,
        from: &str,
        raw: &str,
        kind: EdgeKind,
        rel: &str,
        span: proc_macro2::Span,
        mod_id: &str,
    ) {
        self.raw_edges.push(RawEdge {
            from: from.into(),
            raw: raw.into(),
            kind,
            file: rel.into(),
            line: span.start().line as u32,
            module: mod_id.into(),
        });
    }

    fn push(&mut self, s: Symbol) {
        self.symbols.push(s);
    }

    // ------------------------------------------------------------- resolve

    fn resolve(&mut self) -> (Vec<Edge>, BTreeMap<String, usize>) {
        let index: BTreeSet<String> = self.symbols.iter().map(|s| s.id.clone()).collect();
        let mods: BTreeSet<String> = self
            .modules
            .iter()
            .map(|m| m.id.clone())
            .filter(|m| m != "(crate)")
            .collect();

        let mut externs: BTreeMap<String, usize> = BTreeMap::new();
        let mut edges = Vec::new();
        let mut unresolved: Vec<Diagnostic> = Vec::new();

        // Primitives and common std prelude names are not architecture.
        let ignore: BTreeSet<&str> = [
            "Self", "self", "String", "str", "bool", "usize", "u8", "u16", "u32", "u64", "i8",
            "i16", "i32", "i64", "f32", "f64", "char", "Vec", "Option", "Some", "None", "Result",
            "Ok", "Err", "Box", "HashMap", "HashSet", "BTreeMap", "BTreeSet", "PathBuf", "Path",
            "Duration", "Default", "Clone", "Copy", "Debug", "PartialEq", "Eq", "Hash", "Ord",
            "PartialOrd", "From", "Into", "Iterator", "ToString", "Display", "Drop", "Send",
            "Sync", "Sized", "Fn", "FnMut", "FnOnce", "Cow", "Rc", "Arc", "RefCell", "Cell",
            "Mutex", "RwLock", "OnceCell", "Ordering", "SystemTime", "Instant",
        ]
        .into_iter()
        .collect();

        for re in &self.raw_edges {
            let uses = self
                .mod_ctx
                .get(&re.module)
                .map(|c| &c.uses)
                .cloned()
                .unwrap_or_default();

            match resolve_path(&re.raw, &re.module, &uses, &index, &mods) {
                Res::Local(id) => edges.push(Edge {
                    from: re.from.clone(),
                    to: EdgeTarget::Local { id },
                    kind: re.kind,
                    file: re.file.clone(),
                    line: re.line,
                    resolved: true,
                }),
                Res::Path(full) => {
                    let head = full.split("::").next().unwrap_or("").to_string();
                    let is_extern = BUILTIN_CRATES.contains(&head.as_str())
                        || self.extern_crates.contains(&head);
                    if is_extern {
                        *externs.entry(extern_key(&full)).or_insert(0) += 1;
                        edges.push(Edge {
                            from: re.from.clone(),
                            to: EdgeTarget::Extern {
                                krate: head,
                                path: full,
                            },
                            kind: re.kind,
                            file: re.file.clone(),
                            line: re.line,
                            resolved: true,
                        });
                    } else if is_architectural(&full, &ignore) {
                        unresolved.push(Diagnostic {
                            kind: DiagnosticKind::UnresolvedPath,
                            file: re.file.clone(),
                            line: re.line,
                            detail: format!("`{}` in {}", full, re.module),
                        });
                    }
                }
            }
        }

        // Bind impls to their trait/type ids now that the index exists.
        let mods_c = mods.clone();
        let ctx = std::mem::take(&mut self.mod_ctx);
        for b in &mut self.impls {
            let uses = ctx.get(&b.module).map(|c| c.uses.clone()).unwrap_or_default();
            b.trait_id = match resolve_path(&b.trait_path, &b.module, &uses, &index, &mods_c) {
                Res::Local(id) => Some(id),
                Res::Path(_) => None,
            };
            if b.type_id.as_deref().map(|i| !index.contains(i)).unwrap_or(true) {
                b.type_id = None;
            }
        }
        self.mod_ctx = ctx;

        self.diagnostics.extend(unresolved);
        (edges, externs)
    }

    fn rel(&self, p: &Path) -> String {
        p.strip_prefix(&self.root)
            .unwrap_or(p)
            .display()
            .to_string()
    }

    fn module_file(
        &self,
        parent_file: &Path,
        _mod_id: &str,
        name: &str,
        is_root: bool,
        attrs: &[syn::Attribute],
    ) -> Option<PathBuf> {
        for a in attrs {
            if a.path().is_ident("path") {
                if let syn::Meta::NameValue(nv) = &a.meta {
                    if let syn::Expr::Lit(l) = &nv.value {
                        if let syn::Lit::Str(s) = &l.lit {
                            let p = parent_file.parent()?.join(s.value());
                            if p.exists() {
                                return Some(p);
                            }
                        }
                    }
                }
            }
        }
        let dir = parent_file.parent()?;
        let stem = parent_file.file_stem()?.to_str()?;
        // A crate root or `mod.rs` owns its own directory; `foo.rs` owns `foo/`.
        let base = if is_root || stem == "mod" {
            dir.to_path_buf()
        } else {
            dir.join(stem)
        };
        for cand in [base.join(format!("{name}.rs")), base.join(name).join("mod.rs")] {
            if cand.exists() {
                return Some(cand);
            }
        }
        None
    }
}

// ------------------------------------------------------------------ helpers

/// The outcome of resolving one path.
pub enum Res {
    /// Resolved to a symbol or module in this crate.
    Local(String),
    /// Expanded as far as we can, but not local. The caller decides whether
    /// the head names a dependency (an extern edge) or nothing we know
    /// (a diagnostic). Expanding first is what makes `Style` resolvable to
    /// `ratatui::style::Style` via the file's `use` map.
    Path(String),
}

fn resolve_path(
    raw: &str,
    module: &str,
    uses: &HashMap<String, String>,
    index: &BTreeSet<String>,
    mods: &BTreeSet<String>,
) -> Res {
    let raw = raw.trim_start_matches("::");
    let segs: Vec<&str> = raw.split("::").collect();
    let head = match segs.first() {
        Some(h) => *h,
        None => return Res::Path(raw.into()),
    };
    let rest = |n: usize| segs[n..].join("::");

    let candidate = match head {
        // `crate`/`self`/`super` are local by construction. If they do not
        // resolve, the answer is "unknown", never "extern".
        "crate" => rest(1),
        "self" => join(module, &rest(1)),
        "super" => join(&parent_of(module).unwrap_or_default(), &rest(1)),
        _ => {
            if let Some(mapped) = uses.get(head) {
                let local = mapped.trim_start_matches("crate::").to_string();
                let expanded = if segs.len() == 1 { local } else { join(&local, &rest(1)) };
                match lookup(&expanded, index, mods) {
                    Some(id) => return Res::Local(id),
                    // Not local, so the `use` pointed outside the crate. The
                    // expanded path carries the real head (`ratatui`, `std`).
                    None => return Res::Path(expanded),
                }
            }
            // Try the path as a sibling of the current module. The walk-back
            // in `lookup` must not chew through the module prefix itself:
            // `std::fs::metadata` inside module `app` becomes the candidate
            // `app::std::fs::metadata`, and collapsing that to `app` would
            // turn every std call in the file into a self-edge.
            let local = join(module, raw);
            let floor = if module.is_empty() {
                1
            } else {
                module.split("::").count() + 1
            };
            if let Some(id) = lookup_min(&local, index, mods, floor) {
                return Res::Local(id);
            }
            raw.to_string()
        }
    };

    match lookup(&candidate, index, mods) {
        Some(id) => Res::Local(id),
        None => Res::Path(candidate),
    }
}

/// Exact hit, else walk back up the path: `app::App::new` is the type `app::App`.
fn lookup(path: &str, index: &BTreeSet<String>, mods: &BTreeSet<String>) -> Option<String> {
    lookup_min(path, index, mods, 1)
}

/// As `lookup`, but never returns a path shorter than `min_segs` segments.
fn lookup_min(
    path: &str,
    index: &BTreeSet<String>,
    mods: &BTreeSet<String>,
    min_segs: usize,
) -> Option<String> {
    if index.contains(path) || mods.contains(path) {
        return Some(path.to_string());
    }
    let mut probe = path.to_string();
    while let Some(pos) = probe.rfind("::") {
        probe.truncate(pos);
        if probe.split("::").count() < min_segs {
            return None;
        }
        if index.contains(&probe) || mods.contains(&probe) {
            return Some(probe);
        }
    }
    None
}

/// Which unresolved paths are worth a diagnostic.
///
/// A bare lowercase single-segment name that did not resolve locally is a
/// prelude function, a closure, or a local binding — `drop(x)`, `f()`. None of
/// those are architecture, and reporting each one buries the paths that are.
/// A qualified path or a type-shaped name that we could not place is real.
fn is_architectural(path: &str, ignore: &BTreeSet<&str>) -> bool {
    let head = match path.split("::").next() {
        Some(h) if !h.is_empty() => h,
        _ => return false,
    };
    if ignore.contains(head) {
        return false;
    }
    let qualified = path.contains("::");
    let type_shaped = head.chars().next().is_some_and(|c| c.is_uppercase());
    qualified || type_shaped
}

fn flatten_use(
    tree: &syn::UseTree,
    prefix: String,
    out: &mut Vec<(String, String)>,
    on_glob: &mut impl FnMut(&str),
) {
    match tree {
        syn::UseTree::Path(p) => {
            let next = join(&prefix, &p.ident.to_string());
            flatten_use(&p.tree, next, out, on_glob);
        }
        syn::UseTree::Name(n) => {
            let name = n.ident.to_string();
            if name == "self" {
                // `use a::b::{self, C}` brings in `a::b` under the name `b`.
                if !prefix.is_empty() {
                    let alias = last_seg(&prefix).to_string();
                    out.push((prefix, alias));
                }
            } else {
                out.push((join(&prefix, &name), name));
            }
        }
        syn::UseTree::Rename(r) => {
            out.push((join(&prefix, &r.ident.to_string()), r.rename.to_string()));
        }
        syn::UseTree::Glob(_) => on_glob(&prefix),
        syn::UseTree::Group(g) => {
            for t in &g.items {
                flatten_use(t, prefix.clone(), out, on_glob);
            }
        }
    }
}

fn fields_of(f: &syn::Fields) -> Vec<Field> {
    match f {
        syn::Fields::Named(n) => n
            .named
            .iter()
            .map(|f| Field {
                name: f.ident.as_ref().map(|i| i.to_string()).unwrap_or_default(),
                ty: type_to_string(&f.ty),
                ty_paths: paths_in_type(&f.ty),
            })
            .collect(),
        syn::Fields::Unnamed(u) => u
            .unnamed
            .iter()
            .enumerate()
            .map(|(i, f)| Field {
                name: i.to_string(),
                ty: type_to_string(&f.ty),
                ty_paths: paths_in_type(&f.ty),
            })
            .collect(),
        syn::Fields::Unit => vec![],
    }
}

fn sig_of(s: &syn::Signature) -> FnSig {
    let mut params = Vec::new();
    let mut takes_self = false;
    for a in &s.inputs {
        match a {
            syn::FnArg::Receiver(_) => takes_self = true,
            syn::FnArg::Typed(t) => params.push(Param {
                name: match &*t.pat {
                    syn::Pat::Ident(i) => i.ident.to_string(),
                    _ => "_".into(),
                },
                ty: type_to_string(&t.ty),
                ty_paths: paths_in_type(&t.ty),
                bounds: bounds_in_type(&t.ty),
            }),
        }
    }
    let (ret, ret_paths) = match &s.output {
        syn::ReturnType::Default => (None, vec![]),
        syn::ReturnType::Type(_, t) => (Some(type_to_string(t)), paths_in_type(t)),
    };
    FnSig {
        params,
        ret,
        ret_paths,
        generic_bounds: generic_bounds(&s.generics),
        is_async: s.asyncness.is_some(),
        takes_self,
    }
}

fn vis_of(v: &syn::Visibility) -> Visibility {
    match v {
        syn::Visibility::Public(_) => Visibility::Public,
        syn::Visibility::Restricted(r) => {
            if r.path.is_ident("crate") {
                Visibility::Crate
            } else if r.path.is_ident("super") {
                Visibility::Super
            } else {
                Visibility::Crate
            }
        }
        syn::Visibility::Inherited => Visibility::Private,
    }
}

fn derives(attrs: &[syn::Attribute]) -> Vec<String> {
    let mut out = Vec::new();
    for a in attrs {
        if a.path().is_ident("derive") {
            let _ = a.parse_nested_meta(|m| {
                if let Some(i) = m.path.get_ident() {
                    out.push(i.to_string());
                }
                Ok(())
            });
        }
    }
    out
}

fn attr_names(attrs: &[syn::Attribute]) -> Vec<String> {
    attrs
        .iter()
        .map(|a| path_string(a.path()))
        .filter(|n| n != "doc")
        .collect()
}

fn cfgs(attrs: &[syn::Attribute]) -> Vec<String> {
    use quote::ToTokens;
    attrs
        .iter()
        .filter(|a| a.path().is_ident("cfg"))
        .map(|a| a.meta.to_token_stream().to_string())
        .collect()
}

fn has_cfg_test(attrs: &[syn::Attribute]) -> bool {
    cfgs(attrs).iter().any(|c| c.contains("test"))
}

fn is_test_fn(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        let p = path_string(a.path());
        p == "test" || p.ends_with("::test")
    })
}

fn span_loc(s: proc_macro2::Span) -> u32 {
    let (a, b) = (s.start().line, s.end().line);
    (b.saturating_sub(a) + 1) as u32
}

fn join(a: &str, b: &str) -> String {
    match (a.is_empty(), b.is_empty()) {
        (true, _) => b.to_string(),
        (_, true) => a.to_string(),
        _ => format!("{a}::{b}"),
    }
}

fn parent_of(id: &str) -> Option<String> {
    id.rfind("::").map(|i| id[..i].to_string())
}

fn last_seg(p: &str) -> &str {
    p.rsplit("::").next().unwrap_or(p)
}

fn extern_key(path: &str) -> String {
    let segs: Vec<&str> = path.split("::").collect();
    if BUILTIN_CRATES.contains(&segs[0]) && segs.len() > 1 {
        format!("{}::{}", segs[0], segs[1])
    } else {
        segs[0].to_string()
    }
}

fn read_manifest(p: &Path) -> Result<(String, BTreeSet<String>)> {
    let src = std::fs::read_to_string(p)
        .with_context(|| format!("no Cargo.toml at {}", p.display()))?;
    let v: toml::Value = src.parse().context("Cargo.toml is not valid TOML")?;
    let name = v
        .get("package")
        .and_then(|p| p.get("name"))
        .and_then(|n| n.as_str())
        .unwrap_or("unknown")
        .to_string();
    let mut deps = BTreeSet::new();
    for table in ["dependencies", "dev-dependencies", "build-dependencies"] {
        if let Some(t) = v.get(table).and_then(|d| d.as_table()) {
            for k in t.keys() {
                deps.insert(k.replace('-', "_"));
            }
        }
    }
    Ok((name, deps))
}
