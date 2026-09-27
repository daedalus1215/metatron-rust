---
title: Symbol Model and the `syn` Scanner
status: implemented
project: metatron-rust
location: specs/01-symbol-model.md
created: 2026-08-29
tags: [scanner, syn, model, foundation]
implemented: 2026-08-29
---

# Symbol Model and the `syn` Scanner

## Context

metatron-nestjs models a codebase as **files and imports**. That works for
NestJS because one file holds one class holds one architectural unit — the
coincidence that makes a file-level import graph meaningful.

Rust breaks the coincidence. `arioch/src/app.rs` is 1,973 lines holding five
types and 48 methods on one struct, and its entire outbound coupling is three
`use crate::` lines. A file-level graph draws it as one box with three arrows
and reports a clean architecture. Everything worth knowing is inside the box.

Worse, the architecture in `patterns-rust` lives in places a file graph cannot
represent at all. `impl ActivityStore for SqliteActivityStore` binds an infra
struct to a domain trait — the dependency-inversion arrow the whole hexagonal
design rests on — and it produces **no import edge whatsoever**. Neither does
`fn start_activity(store: &impl ActivityStore)`, which is how the glossary says
a use case reaches the outside world.

So the node has to be the symbol, and `impl` has to be an edge.

## Goal

Parse a Rust crate into a symbol-level model that can express layer membership,
the port seam, and intra-type cohesion — and that declares what it failed to
understand rather than guessing.

## Design

### Why `syn`, and not the alternatives

| approach | verdict |
|---|---|
| **regex** | Rejected. metatron's TS scanner counts `<` and `>` to find method bodies. Rust has lifetimes `<'a>`, turbofish `::<>`, `where` clauses, and `Vec<Box<dyn Fn() -> Result<(), E>>>`. It would be wrong on the first generic. |
| **`rust-analyzer` as a library** | Rejected for now. It gives true name resolution and real call graphs — genuinely better data. But `ra_ap_*` crates are unstable, unversioned against releases, heavyweight, and the API is undocumented. Revisit if best-effort resolution proves insufficient; the model schema below is deliberately resolver-agnostic so the front end can be swapped. |
| **`syn` + a hand-built symbol table** | Chosen. Stable, fast, well-documented, parses the real grammar. Costs us name resolution, which is handled explicitly below. |

`syn` with `features = ["full", "extra-traits", "visit"]`, plus
`proc-macro2` for spans (line numbers), and `cargo_metadata` to find crate roots
and distinguish workspace members from third-party dependencies.

### Nodes

Three levels, with containment:

```
module  (one .rs file, or an inline `mod {}`)
  ├─ type   struct | enum | trait | union | type alias
  │    └─ fn     method, via an impl block
  └─ fn     free function
```

```rust
pub struct Symbol {
    pub id: String,            // "app::App" | "app::App::refresh_content" | "app"
    pub kind: SymbolKind,      // Module | Struct | Enum | Trait | Union | TypeAlias | Fn | Method
    pub name: String,
    pub module: String,        // owning module path
    pub parent: Option<String>,// owning type, for methods
    pub file: String,          // root-relative
    pub line: u32,
    pub vis: Visibility,       // Private | Crate | Super | Public
    pub cfg: Vec<String>,      // raw #[cfg(..)] predicates in scope
    pub is_test: bool,         // under #[cfg(test)] or a #[test] fn
    pub attrs: Vec<String>,    // derives and other attributes, as written
    pub fields: Vec<Field>,    // structs/enums only
    pub sig: Option<FnSig>,    // fns/methods only
    pub loc: u32,              // source lines, for weighting
}
```

`Visibility` is first-class because `crate-graph.md` names it as an enforcement
mechanism in the single-crate case: "items that must not leak are `pub(crate)`
or in private modules." A rule cannot check that unless the model carries it.

### Edges

Six kinds. Only the first exists in metatron today.

| kind | source | why it matters |
|---|---|---|
| `Use` | `use crate::x::Y` | the classic import edge; module-level coupling |
| `Impl` | `impl Trait for Type` | **the port seam.** Points from the concrete to the trait — backwards against layer order. This is dependency inversion, and it is the one edge with no NestJS analog. |
| `Field` | `struct A { b: B }` | structural composition; also the input to LCOM4 (spec 02) |
| `Sig` | fn params and return types | how a use case names its ports |
| `Bound` | `T: ActivityStore`, `&impl Store`, `&dyn Store` | **calls through the port.** Distinguishing `&impl ActivityStore` from `&SqliteActivityStore` is precisely rule `concrete-outside-root` in spec 04. |
| `Call` | `foo()` / `self.bar()` in a body | needed for same-level violations (use-case → use-case) and for service-wraps-one |

```rust
pub struct Edge {
    pub from: String,          // Symbol id
    pub to: EdgeTarget,        // Local(String) | Extern { krate, path }
    pub kind: EdgeKind,
    pub file: String,
    pub line: u32,
    pub resolved: bool,
}
```

`Extern` targets are not noise to be filtered — they are load-bearing. Every
domain-purity rule is "does any symbol in `domain/` have an `Extern` edge to
`rusqlite` / `ratatui` / `crossterm` / `std::fs` / `std::process`". Record the
crate name and the full path.

### Name resolution, and its limits

`syn` parses one file at a time and performs **no name resolution**. Real Rust
resolution needs `use` aliases, glob imports, `crate`/`self`/`super` prefixes,
re-exports, prelude items, macro-expanded names, and trait method dispatch. That
is a compiler's job.

The pragmatic approach, in order:

1. Walk every `.rs` file under the crate root; build the module tree from `mod`
   declarations and directory layout (`foo.rs`, `foo/mod.rs`, `foo/bar.rs`).
2. Collect every item declaration into a symbol table keyed by module path.
3. Resolve each file's `use` statements against that table, recording aliases
   (`use x::Y as Z`) and marking glob imports (`use x::*`) as low-confidence.
4. Resolve every path expression against, in order: local bindings, the file's
   `use` map, the current module, the crate root, then known extern crates from
   `cargo_metadata`.
5. **Anything still unresolved becomes a `Diagnostic`, never a guess.**

```rust
pub struct Diagnostic {
    pub kind: DiagnosticKind,  // UnresolvedPath | GlobImport | MacroItem | CfgExcluded | ParseFailure
    pub file: String,
    pub line: u32,
    pub detail: String,
}
```

This is metatron's existing instinct — its `diagnostics` field exists so an
unparseable route is visible rather than dropped — carried over to a language
where the need is greater.

**Method call resolution is deliberately shallow.** `self.foo()` resolves within
the enclosing type. A call on a generic (`store.totals(..)` where
`store: &impl ActivityStore`) resolves to the *trait method*, which is the
correct and useful answer. A call on a concrete local type resolves to that
type's inherent method. Anything else — a call through a trait object obtained
at runtime, a call on a third-party type — is recorded as unresolved. Do not
attempt trait dispatch; that is where a hand-rolled resolver goes wrong quietly.

### Blind spots, declared up front

These are permanent limits of the approach, and each gets a `Diagnostic` so the
model reports its own ignorance:

- **Macros are opaque.** `syn` does not expand them. Items produced by
  `macro_rules!` or a proc macro do not exist in the model. `#[derive(..)]` is
  captured as an *attribute* (which is enough — the glossary's value-object rule
  wants to see `#[derive(Clone, Copy, PartialEq)]`), but derived trait impls are
  not `Impl` edges. A `#[derive(Serialize)]` produces no edge to `serde`.
- **`#[cfg]` is not evaluated.** Items are included with their predicates
  recorded. `#[cfg(test)]` items are kept and flagged `is_test`, because rule
  `port-has-fake` in spec 04 depends on finding them. Other cfg-gated code is
  included and tagged; a `CfgExcluded` diagnostic notes that some of the model
  may not compile on this target.
- **Imports are not calls, still.** metatron's caveat survives. A `Use` edge is
  a declaration of availability. `Call` edges are the real thing and are
  best-effort.
- **`build.rs` and generated code** are out of scope.

### Model output

`.metatron/model.json`, serde-serialized, deliberately keeping metatron's
existing top-level shape where the meaning carries over — `stats`, `coverage`,
`findings`, `diagnostics`, `violations`, `churn` — so the templates in spec 06
and the baseline logic in spec 05 need the minimum adaptation.

New or changed:

```jsonc
{
  "symbols":  [ /* Symbol */ ],
  "edges":    [ /* Edge */ ],
  "modules":  [ /* module tree with containment */ ],
  "impls":    [ /* trait -> concrete bindings, extracted for convenience */ ],
  "externs":  { "rusqlite": 14, "ratatui": 88, "std::fs": 9 },
  "cohesion": [ /* spec 02 */ ],
  "diagnostics": [ /* Diagnostic */ ]
}
```

`fileNodes` / `fileLinks` are retained as a **projection** of the symbol graph —
collapse symbols to their module, dedupe edges — so metatron's existing views
keep working while the symbol-level views are built in spec 06.

### Churn

Unchanged from metatron: `git log --numstat` if the tree is in a repo,
`churnSince` to bound it, silently skipped otherwise with `churnMeta.available`
saying which. It is language-agnostic and already works.

## Commands

```bash
metatron scan [path]      # parse to .metatron/model.json, no analysis
metatron --help
```

Everything else arrives in later specs.

## Out of scope

- Classification into layers (spec 03) — the scanner assigns no meaning.
- Any rule evaluation (spec 04).
- Views (spec 06).
- Multi-crate workspaces. Parse the crate at the given path. Workspace support
  matters when `crate-graph.md` Option B happens, and it is a resolution
  question (cross-crate paths), not a model question. The `Extern` edge already
  distinguishes a workspace sibling from a third-party crate via
  `cargo_metadata`, which is the hook for it.

## Acceptance

- `metatron scan ~/…/arioch` completes and emits a model with 8 modules.
- `App` appears as one `Struct` symbol with **41 fields** and **48 methods**
  attached as `Method` symbols with `parent: "app::App"`.
- arioch's 5 crate-local `use` statements produce **9** `Use` edges — a leaf
  per imported name, since `use crate::registry::{Entry, Registry}` couples to
  two symbols, not one. `main.rs`'s 7 `mod` declarations produce the module
  tree, not edges.
- `externs` reports `ratatui`, `crossterm`, `std::fs`, and `std::process`
  against the symbols that actually name them — including
  `std::fs::read_to_string` inside `app.rs`, which `adapter.md` calls arioch's
  defining violation.
- No `ImplBinding` in arioch resolves a `trait_id`. arioch has two
  `impl Default for _` blocks, so `impls` is *not* empty — but `Default` is not
  defined in the crate, and **zero locally-defined traits is the correct
  answer**. The port-seam signal is therefore `trait_id.is_some()`, not
  `impls.is_empty()`; a std-trait impl must not be mistaken for a port.
  enoch has neither: 0 traits, 0 impls.
- A file with a deliberate `Vec<Box<dyn Fn() -> Result<(), E>>>`, a `where`
  clause, a turbofish, and a `r#"raw string"#` parses with no diagnostics.
- A `macro_rules!`-generated struct produces a `MacroItem` diagnostic, not a
  silent omission.
- arioch scans with **zero** diagnostics. Every path in it resolves to a local
  symbol or a named dependency. A resolver that cannot place `Style` — imported
  via `use ratatui::style::Style` and then written bare 147 times — is not
  resolving, it is counting.
- Deleting a `use` statement changes the edge count by exactly one.

---

## Implementation notes (2026-08-29)

`src/model.rs` (types), `src/scan/mod.rs` (module tree, extraction, resolution),
`src/scan/types.rs` (type walking), `src/scan/body.rs` (field access and calls).
13 tests in `tests/scan.rs`, two fixture crates.

**Three acceptance criteria in this spec were wrong, and the scanner was right.**

- `App` has **41** fields, not 42. The hand-count that produced the number was
  off by one. Corrected above and in spec 02.
- The `use crate::` criterion counted *statements* and asserted *edges*. Five
  crate-local `use` statements produce nine edges, because
  `use crate::registry::{Entry, Registry}` couples to two symbols.
- `impls` is **not** empty for arioch. It has two `impl Default for _` blocks.
  The spec conflated "no traits defined" with "no traits implemented"; only the
  first is true. The port-seam signal is `trait_id.is_some()` — a resolved,
  locally-defined trait — and the test now asserts that instead.

**Two resolution bugs, both of which produced confidently wrong output rather
than an error**, which is the failure mode this project exists to avoid:

1. *Extern detection ran on the raw path, not the expanded one.* `Style` in
   `ui.rs` is `ratatui::style::Style` via the file's `use` map, but the head
   tested against the dependency list was `Style`. Result: 487 spurious
   `UnresolvedPath` diagnostics and a `ratatui` count of 26 instead of 482.
   `resolve_path` now returns `Res::Local | Res::Path`, where `Path` carries the
   *expanded* path so the caller tests the real head.

2. *The path walk-back chewed through the module prefix.* `std::fs::metadata`
   inside module `app` became the candidate `app::std::fs::metadata`, and the
   walk-back that exists to map `app::App::new` → `app::App` collapsed it all
   the way to `app`. Every std call in a file silently became a self-edge, and
   `app.rs`'s `std::fs` and `std::process` usage — the violation
   `adapter.md` names — vanished entirely. `lookup_min` now takes a floor.

The second bug is the more instructive one: the model looked *healthier* for
having it. arioch appeared to have less I/O in its application layer than it
does. A scanner's bugs are not neutral; they tend toward flattering the code.

**`ImplBinding` gained a `module` field.** The trait in `impl ActivityStore for
SqliteActivityStore` is written bare and resolves only through *that* module's
`use` map; the first implementation resolved it with an empty map and produced
`trait_id: None` for every real port.

**`SymbolKind` gained `Static` and `Const`**, deviating from the eight kinds
specified. `no-global-mut` (spec 04) has to see `static CONFIG_OVERRIDE:
LazyLock<Mutex<Option<PathBuf>>>`, and that is not reachable through any other
kind. It is captured with its type, and the spec 04 rule now has its fixture.

**Measured.** arioch: 8 modules, 166 symbols, 792 edges, 0 diagnostics, ~30 ms
release. enoch: 5 modules, 164 symbols, 692 edges, 0 diagnostics. Both report
0 traits. `tui::State` (21 fields, 30 methods) and `db::Db` (24 methods) join
`App` as spec 02 subjects.

One finding fell out of the scan for free: **`App::selected_category` is
declared and never touched** by any of the 48 methods through `self`. 40 of 41
fields are used. That is spec 02's unused-field case, confirmed before spec 02
exists.
