---
title: Cohesion Analysis (LCOM4)
status: implemented
project: metatron-rust
location: specs/02-cohesion-analysis.md
created: 2026-08-29
tags: [cohesion, lcom, srp, god-object, new-analysis]
---

# Cohesion Analysis (LCOM4)

## Context

The intuition this spec formalises: *a file holding functions and types and
everything else is doing too much, and the tool should say so.*

The intuition is right, but the obvious version of it does not work. Every Rust
file contains types and functions — that is what a Rust file is. "Has both" is
not a signal; it is a tautology. The signal has to be sharper, and it has to be
computed rather than eyeballed.

There is a second reason this spec comes before classification. Every other
analysis in this project needs a codebase that follows `patterns-rust`, and
neither target codebase does. Cohesion analysis needs **only the parser**. It
runs against unrefactored code and produces its most valuable output precisely
when conformance is zero — which is the situation today. It is the one thing
metatron-rust can say about arioch on day one.

And it does something no conformance rule can: it does not merely flag the god
object, **it names the seam where it splits**. For a tool whose job is to
measure a refactor, a finding that proposes the next commit is worth more than
one that grades the last.

## Goal

Detect single-responsibility violations at three granularities, and for the type
case, output a concrete decomposition proposal.

## Design

### Detector 1 — LCOM4 on impl blocks

The core measure. **Lack of Cohesion of Methods**, variant 4: build a graph
whose vertices are a type's methods and fields; connect a method to every field
it touches, and to every sibling method it calls. The number of **connected
components** is LCOM4. One component means a cohesive type. *N* components means
*N* types wearing one name.

```
for each type T with methods:
    V = methods(T) ∪ fields(T)
    E = { (m, f) : m reads or writes self.f }
      ∪ { (m, n) : m calls self.n() }
    lcom4 = count_connected_components(V, E)
```

Field access is collected during the `syn` visit in spec 01: every `self.<ident>`
expression in a method body, whether read, written, borrowed, or passed. Method
calls (`self.foo()`) join components and must be included, or any type with a
private helper called from two places reports a false split.

**Fields touched by no method** form singleton components. Report them
separately — dead state is its own finding, and it inflates LCOM4 in a way that
is true but not actionable in the same way.

### The proposal

A bare number is not useful. For each component, emit:

- its methods and fields
- a **suggested name**, derived from the longest common prefix of the component's
  field names where one exists (`suggestion_selected`, `suggestion_scroll`,
  `suggestion_filter` → `Suggestion`), and otherwise from the dominant verb stem
  in the method names
- its total LOC, so the size of the extraction is visible
- **cross-component edges**, which are the ones that will have to become
  parameters when the type is split. This is the cost of the refactor, stated
  before it starts.

That last item is what makes the output a plan rather than a complaint.

### Worked example: arioch `App`

`app.rs` declares `pub struct App` with **41 fields**, and `impl App` carries
**48 methods**. The field names alone already suggest the shape, which makes it
an ideal fixture — the answer can be checked by hand before trusting the
detector:

```
search_*                                                    2 fields
suggestion_*                                                4
map_*                                                       3
investigate_*                                               3
annot_* + annotations                                       5
bulk_prompt, bulk_input, multi_selected                     3
file_content, baseline_content, baseline_entry,
  show_diff, file_error                                     5
sidebar_expanded, sidebar_width                             2
edit, view_line                                             2
core: config, registry, mode, selected_entry,
  selected_category, message, show_help, quit,
  scroll_offset, dialog, last_mtime, last_index_mtime      12
```

Roughly **eight to ten components**. Note that the prefix grouping above is a
hand-reading, not the algorithm — the detector must reach a similar answer from
*field access*, not from names, or it is a naming lint pretending to be a
cohesion metric. A type whose fields are named `a`, `b`, `c` must still
decompose correctly. Names are used only to *label* a component once found.

`config` and `registry` will almost certainly bind many components together into
one large blob, because nearly every method touches them. That is not a defect
in the metric — it is the finding. Those two are the type's genuine shared
dependencies and should become constructor parameters of each extracted type,
which is exactly what the cross-component edge list will say.

### Detector 2 — mixed-layer module

Needs spec 03, so it ships in the same pass but stays dark until a config
exists. Classify each **symbol**, not each file. A module whose symbols
classify to more than one layer is incohesive by the glossary's own definition,
since `design-philosophy.md` puts "colocate what changes together" at the centre
of the structured-design contribution.

`arioch/src/app.rs` should report three layers: `App` and the key handlers
(application), `Mode` / `EditState` / `DialogState` (domain value objects), and
`std::fs::read_to_string` plus `$EDITOR` spawning (infrastructure).

### Detector 3 — mixed-concern function

A single function whose `Extern` fan-out reaches more than one concern group —
`{fs, process}` and `{ratatui, crossterm}` in the same body means it does I/O
and rendering. Cheap to compute once `Extern` edges carry the calling symbol,
and it localises detector 2 down to the exact function.

## Thresholds and gating

Thresholds are judgment, and a ratchet built on judgment is how a tool gets
switched off. So:

| finding | tone | gated |
|---|---|---|
| `lcom4` components > 1 | note | no |
| `lcom4` components ≥ 4 **and** methods ≥ 20 | warn | **no** |
| unused field | note | no |
| mixed-layer module | warn | yes (spec 04) |
| mixed-concern fn | note | no |

**Nothing in this spec gates a build.** Cohesion is advisory in every form. The
mixed-layer finding is gated only because it is decided by the classifier, not
by a threshold — it is a layering rule that happens to be computed here.

This follows metatron's existing `gate: false` convention for aggregate
findings, and the reasoning is the same: these are worth printing and meaningless
to ratchet.

## Output

```
metatron cohesion · arioch

  App                          app.rs:31       42 fields  48 methods
    9 components  !
      Suggestion   4 fields   6 methods   112 loc
      Annotation   5 fields   7 methods   208 loc
      Investigate  3 fields   4 methods    96 loc
      Map          3 fields   3 methods    74 loc
      Search       2 fields   4 methods    61 loc
      Bulk         3 fields   5 methods    88 loc
      FileView     5 fields   6 methods   154 loc
      Sidebar      2 fields   2 methods    23 loc
      (core)      12 fields  11 methods   402 loc

    shared across components: config, registry
      -> pass to each extracted type; 31 cross-component edges

  Registry                     registry.rs:24   2 fields  14 methods
    1 component — cohesive
```

The numbers above are illustrative. The acceptance criteria below are not.

## Model changes

```jsonc
"cohesion": [{
  "symbol": "app::App",
  "file": "app.rs", "line": 31,
  "fields": 42, "methods": 48,
  "components": [{
    "name": "Suggestion",
    "fields": ["suggestion_selected", "suggestion_scroll", …],
    "methods": ["filter_suggestions", …],
    "loc": 112
  }],
  "shared": ["config", "registry"],
  "crossEdges": 31,
  "unusedFields": []
}]
```

## Out of scope

- Applying the split. This reports; it does not refactor.
- Cohesion of modules by co-change (that is metatron's logical-coupling spec 03,
  a git-history analysis, and it is orthogonal).
- Enum variants. LCOM4 is defined over a type's fields; enums are analysed only
  when they carry data and have methods.

## Acceptance

- `selected_category` is **not** reported as an unused field. It is declared
  on `App`, never touched through `self` by any method, and read in `ui.rs`
  as `app.selected_category`. *(This criterion originally asserted the
  opposite. See the implementation notes.)*
- Against arioch `App`: reports **more than one** component, with `suggestion_*`
  fields landing together, `annot_*` together, `map_*` together, and
  `investigate_*` together. The exact component count is not asserted — `config`
  and `registry` legitimately merge groups — but those four clusters must not be
  split across components.
- Records and enums are absent from the report entirely.
- `Counter::tally`, assigned twice and read nowhere, is reported write-only;
  `Counter::total`, assigned and read, is not.
- The same result is produced when every field of `App` is renamed to
  `f0..f40`. **This is the test that proves the detector reads access, not
  names.** Only the suggested labels may differ.
- `Registry` (2 fields, 14 methods) reports as cohesive or near-cohesive; a
  detector that calls arioch's most reasonable type a god object is miscalibrated.
- enoch's `tui::State` (21 fields, 30 methods) and `db::Db` (24 methods) are the
  second subject. `Db` holding 24 methods over a single connection field is the
  shape `store.md` predicts, and should *not* decompose — it is a store doing a
  store's job. If the detector splits `Db`, it is measuring size, not cohesion.
- A method calling `self.helper()` where `helper` touches no fields does not
  create a spurious component.
- A field written by exactly one method and read by none is reported as unused,
  not as a component.
- A type whose body contains an unresolved macro invocation emits a diagnostic
  and is **excluded** from the report rather than analysed with partial data —
  a decomposition proposal built on half a type is worse than none.


---

## Implementation notes (2026-08-31)

`src/cohesion.rs`, `metatron cohesion [path] [--all] [--json]`, 15 tests in
`tests/cohesion.rs` and a six-module fixture crate. 68ms over arioch,
including the parse.

### LCOM4 alone does not work, and the failure is instructive

The spec assumed connected components would name the seam. They do not.
Run over arioch's `App`, LCOM4 returns **one component holding 40 of the 41
fields**. The count is 3, but the other two are `selected_category` (dead)
and one inert method — the partition is a single blob.

The spec predicted the cause and named the wrong culprit. It expected
`config` and `registry` to glue everything together. Measured, no field on
`App` is touched by even half the methods:

```
mode 45%   registry 45%   selected_entry 43%   file_content 18%   ...
```

The glue is on the **method** side: `handle_normal` touches 13 fields and
calls 21 siblings. But removing the dispatchers does not help either —
dropping both `handle_normal` and `handle_key` still leaves a 33-field
component. There is no hub to excise. The type is simply dense.

So LCOM4 answers *"is this already several types?"* and nothing more. A
second measure was needed for *"where would it split?"*: greedy modularity
maximisation (Clauset-Newman-Moore) over a method projection weighted by
shared field access. Two numbers, two questions, both reported:

| type | fields | methods | LCOM4 | Q | verdict |
|---|---|---|---|---|---|
| `arioch::App` | 41 | 46 | 3 | 0.091 | **tangled** |
| `enoch::tui::State` | 21 | 29 | 2 | 0.114 | **tangled** |
| `enoch::db::Db` | 1 | 24 | 1 | 0.008 | cohesive |
| `arioch::Registry` | 2 | 17 | 1 | 0.025 | cohesive |

Label propagation was tried first and collapsed to one community, which is
its known behaviour on dense graphs.

### The verdict that was not in the spec

Q for `App` is 0.091, an order of magnitude below the 0.3 at which a
partition is usually considered real structure. The spec's output sketch
showed nine clean components with names; that was wishful. The honest
finding is a fourth verdict the spec did not have:

> **tangled** — wide, and *no clean seam exists*. The three communities
> found share eleven fields across 43 accesses.

This is the worse of the two findings, not a weaker one. A splittable god
object has a next commit. A tangled one does not: nothing lifts out
without dragging shared state along, and the eleven shared fields are the
list of what the refactor has to deal with first.

### The detector accused a live field of being dead

Spec 01 reported `App::selected_category` as declared-and-never-touched,
and this spec made that its first acceptance criterion. **It is read at
`ui.rs:109`.** The field is `pub`; absence from its own type's `self`
accesses proves nothing about a `pub` field.

Two fixes, both conservative in the direction of saying less:

1. `Model.foreign_field_reads` — every field name read through a base
   other than `self`, crate-wide. A field is dead only if its own methods
   never touch it *and* nothing else names it. Name-based and imprecise,
   but it can only suppress a finding, never raise one.
2. `syn` models a macro invocation as an opaque token stream and does not
   descend into it, so **every access inside `format!`, `write!` or `vec!`
   was invisible.** `Annotation::text` is used exactly once in arioch,
   inside a `format!`, and was reported dead. `BodyScan` now walks macro
   tokens directly, matching the shape `ident . ident` and treating a
   following parenthesised group as the difference between a field and a
   call. arioch has no `self.<field>` inside a macro, so the LCOM inputs
   were unaffected — that was luck, not design.

### Everything with fields was being analysed

The first run reported `Entry`, `DialogState`, `CategoryColors`, `Config`
and the `Command` enum as maximally disconnected, with every field
"never touched". All noise, from two causes:

- A struct with **no methods** is a record. *Lack of cohesion of methods*
  is undefined when there are none, and reporting every field of a config
  struct as dead is how a tool teaches people to ignore it.
- An **enum**'s fields belong to variants and are disjoint by
  construction, so LCOM4 over them just recounts the variants.

Three floors now apply, each for a stated reason rather than to quiet the
output: fewer than 5 fields (nothing to partition — this is what protects
`enoch::db::Db`), fewer than 3 methods (`arioch::Config`, a settings
record with two helpers that happen to touch different fields), and no
methods at all.

### Two false positives the fixture exists to catch

- **Constructors.** Spec 01 discarded the receiver, so `App::new` was
  indistinguishable from a method — and having no `self` accesses at all,
  it became an isolated component in every type that has one. `FnSig`
  now carries `takes_self`.
- **Stateless helpers.** The live subgraph, which exists to keep dead
  fields from inflating the count, initially dropped any method with no
  field access and no outgoing call. That deleted `Helper::norm` — and
  its two callers, joined only through it, fell apart into two
  components. This is precisely the false split the spec warned about,
  reintroduced by the fix for a different problem. A method is inert only
  if it also has no *incoming* call.

### Mixed-concern functions: the concern table cannot key on the crate

Detector 3 found nothing at first. The reason is that `std` is one crate
spanning every concern there is, so `std::fs` and `std::process` arrive
as krate `std` and the lookup missed. Grouping `std` by its first module
instead, arioch yields eight findings — including:

```
ui.rs:205   ui::render_main   [io + ui]
```

which calls `std::fs::metadata` while building widgets. `enoch::db::Db::open`
is reported `[io + persistence]`, which is arguably correct behaviour for a
store opening a file-backed database; it is a note, and notes do not gate.

Detector 2 (mixed-layer module) remains dark, as the spec sequenced it:
it needs the spec 03 classifier. *(Implemented under spec 03 as
`Classified::mixed_layer_modules`, since `cohesion::analyse` takes only a
model and the detector needs a config.)*

### Model additions

`FnSig.takes_self`, `Symbol.self_reads` (the subset of `self_fields` whose
value is consumed, which is what separates a write-only field from a dead
one), `Model.foreign_field_reads`, `Model.macro_tainted`.

### What nothing here says

`App` is tangled and `State` is tangled. Neither is a layering finding —
this whole spec runs on the parser alone and knows nothing about
`patterns-rust`. Nothing gates.
