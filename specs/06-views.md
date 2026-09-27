---
title: Retargeting the Views
status: implemented
project: metatron-rust
location: specs/06-views.md
created: 2026-08-29
tags: [views, templates, presentation]
---

# Retargeting the Views

## Context

metatron's six templates are 3,456 of its 5,673 lines, and none of them are
TypeScript-aware. Each is an HTML file with one `__DATA__` token in a JSON
script tag; the build reads the template, substitutes the token and a handful of
`{{project}}`-style placeholders, and writes the result. In Rust that is
`include_str!` plus a string replace, so **the templates port at zero cost** —
they are embedded in the binary rather than shipped as a directory.

What has to change is the payload each one receives, because the model
underneath is now symbols rather than files.

This spec is last for a reason. Against a codebase at 0% conformance, five
lenses render five pictures of the same undifferentiated blob, and the effort
goes into making that blob attractive. Numbers first.

## Goal

Feed the existing templates from the symbol model, retarget the two views whose
subject does not exist in Rust, and add the one view the new model makes
possible.

## Design

### View by view

| view | verdict |
|---|---|
| `atlas` | **works as-is.** Folder graph plus findings. Feed it the module-level projection from spec 01. |
| `hotspots` | **works unchanged.** Git churn against dependents — entirely language-agnostic, and the only view that produces a real result on arioch today. |
| `city` | **retarget, no template change.** Below. |
| `layers` | **retarget, small template change.** Below. |
| `traffic` | **retarget.** Below. |
| `schema` | **defer.** Below. |

### `city` — the free win

The template already renders one tower per module and one floor per directory,
with the isometric projection that makes tower heights comparable by eye. Change
only what fills it:

```
tower = module        floor = type        floor colour = layer
```

A tower whose floors are different colours **is** a mixed-layer module —
spec 02's detector 2, made visible at a glance and needing no legend. Tower
height becomes the type count, which is honest under a parallel projection in
exactly the way metatron's README argues.

No changes to `city.html`. This is the highest ratio of insight to work in the
project.

### `layers` — the inverted arrow

Symbols on the plane of their layer, per the ordering in `metatron.toml`. One
template change is required and it is the important one: **`Impl` edges must be
drawn differently from every other edge.**

`impl ActivityStore for SqliteActivityStore` runs from infrastructure back up to
domain. Drawn like an ordinary dependency it looks like the worst violation on
the diagram; it is in fact the dependency inversion the architecture is built
on. Draw it as a distinct stroke — dashed, or in the `good` colour — and label
the count. A viewer should be able to see the port seam without being told where
to look.

This is also the answer to why file-level tools cannot draw this architecture at
all: that arrow corresponds to no import.

### `traffic` — CLI and TUI, not HTTP

metatron's most striking view animates a request from route to repository.
There are no routes here. But `command-handler.md` names the exact analog:

> **CLI:** a `fn cmd_<verb>(store: &impl Store, …)` per clap subcommand.
> **TUI:** the event loop dispatches to `fn handle_<mode>_key(&mut self, store: &impl Store, key)`.

So an inbound signal is a CLI subcommand or a TUI key in a mode, and the trace
follows `Call` edges from the handler through service → use-case → port, ending
at the impl that satisfies the port. Same view, same animation, different entry
set. The clap `#[derive(Parser)]` structs supply the argument payload the way
DTOs do in the NestJS version.

Two honest limits: `Call` edges are best-effort (spec 01), and a TUI key
dispatch is usually a `match` on a `KeyCode`, so extracting "which key reaches
which use case" means reading match arms. Traces that cannot be resolved go to
`diagnostics` and are **not drawn** — metatron's existing rule that an
unparseable route is reported rather than guessed at.

### `schema` — defer

metatron's schema view reads TypeORM entities and infers implicit foreign keys
from column names. There is no ORM here. The nearest analog is entities, value
objects and projections with their `Field` edges, which is a type diagram — a
real thing, but a different view with a different value proposition, and neither
target codebase has a domain layer to draw yet. Revisit after the arioch
refactor produces one.

Until then the view is **omitted, not emptied**. metatron's build drops
narration slots with nothing to say; the same instinct applies to a whole lens.

### New view — `cohesion`

The one the new model makes possible, and the one with the most to say about
both target codebases today. For each type over threshold, a matrix: methods
down one axis, fields across the other, cells marked where a method touches a
field, rows and columns ordered so connected components fall into visible
blocks. Off-diagonal marks are the cross-component edges — the cost of the
split, drawn.

This is the standard way to present LCOM and it reads instantly: a cohesive type
is one block, `App` will be eight or nine blocks with a smear of shared access
through the `config` and `registry` columns.

### Narration

`narrate.js` (212 lines) ports directly, and the rule it exists to enforce ports
with it: **never type a finding into a template.** Slots are declared as
`<p data-narr="layering"></p>` and filled at build time; slots with nothing to
say are removed.

metatron's README records why, and it is worth restating because the same
mistake is cheap to repeat: the first version had its findings typed into the
HTML, and when the charts updated for a new project the paragraphs kept
confidently describing the old one.

New narration slots for this project: `ports` (how many, how many fakes),
`enforcement` (the compiler/metatron/advisory split), `cohesion`, and
`unevaluable` (which rules could not be judged and why).

### Adapters

metatron's per-view adapters trim the payload — the README records one view
going from 453 KB to 84 KB. The symbol model is substantially larger than the
file model, so this matters more, not less. Each view gets an adapter function;
`traffic` and `cohesion` need only a small projection of the model and must not
receive the full symbol table.

## Out of scope

- New visual styles. Six lenses is enough.
- Interactive filtering beyond what the templates already do.
- `schema`, per above.

## Acceptance

- All six retained templates render from a Rust model with no `__DATA__` token
  left unreplaced and no console errors.
- `city` renders arioch as 8 towers; `app.rs`'s tower shows floors in more than
  one colour once a classification exists.
- `layers` draws `Impl` edges in a visually distinct stroke, and the fixture
  crate's `impl ActivityStore for SqliteActivityStore` is legible as an upward
  arrow that is not a violation.
- `traffic` traces the fixture's `cmd_start` through `start_activity` to
  `ActivityStore::open_session` and stops at the port, with the impl shown as
  the terminus.
- `cohesion` renders `App` with visible blocks matching the components spec 02
  reports.
- `schema` is absent from `index.html` rather than present and empty.
- Every narration slot is either filled from the model or removed. A screenshot
  check per metatron's README — static checks do not catch mirrored text,
  washed-out blends, or a canvas that rendered blank because the virtual-time
  budget was too short.


---

## Implementation notes (2026-08-31)

`src/views/` (pipeline, narration, six adapters), `templates/` (five
vendored plus one written here), `metatron views [path] [name] [--out]`.
13 tests in `tests/views.rs` and `tools/render-check.mjs`. **87 tests
total.**

### "The templates port at zero cost" was half right

The *mechanism* ports exactly as described: one `__DATA__` token, some
`{{project}}` placeholders, `include_str!` and a string replace. The
**contracts** did not. Four separate places hardcode NestJS's model, and
each one was a blank page rather than an error:

| where | what it assumed | what happened |
|---|---|---|
| `atlas.js` `visibleTiers()` | tiers `[0..8]` | `D.tiers[6]` undefined → threw before drawing |
| `layers.js` `CORE` | tiers `[0..7]` | same |
| `layers` links | `[a, b, cross, rule]` **arrays** | I sent objects; `l[0]` was `undefined`, so it drew **nothing at all** |
| `atlas` ports/findings | `p.path`, `c.pattern`, `f.detail` | threw on the first port |

Three template edits in total, each marked `metatron-rust change` in the
file and listed in `templates/NOTICE.md`. Two of them are the same edit:
derive the tier count from the model instead of hardcoding nine.

The link one is the instructive failure. It threw no exception, logged
nothing, and rendered a page that looked structurally complete — correct
header, correct node count, correct legend, and an empty canvas. Both the
static check (no `__DATA__`, JSON parses) and a clean jsdom run reported
it as fine.

So `tools/render-check.mjs` wraps `getContext` and **counts stroke and
fill calls**. `city` and `layers` now register 1,200–4,400 draws per
render; a regression to zero is a failure. This is the one check that
would have caught it, and it is the reason the spec's insistence on a
screenshot step is right — I have not done that step, and it remains the
gap. Mirrored text and washed-out blends survive everything above.

### Measured

Every view renders clean over four crates — `ports`, `mixed`, `leaky`,
`arioch`:

```
              nodes   svg   canvas draws
atlas           261    93        —
city            112     —     3780
cohesion       1995     —        —
hotspots        203    48        —
layers          111     —     4131
traffic         128    22        —
```

- **`city`**: 8 towers for arioch, one per module. `layers`: 5 inversion
  arrows on the fixture, every one running upward, none flagged as a skip.
- **`traffic`**: `cmd_start` → use-case `start_activity` → port
  `ActivityStore`, terminating at the port with the concrete never a hop.
- **`cohesion`**: `App` as 45 rows × 40 columns, 164 marks, three
  contiguous blocks and 43 crossing accesses drawn in the warning colour.

### Three payload decisions the spec did not settle

**A module with no types still gets a tower.** `tower = module` is the
spec's own equation, but a floor is a type, and `arioch::ui` — 1,300
lines of render functions and no types — would have had no tower at all.
Free functions now form their own floor, **one per layer**: taking the
minimum layer would hide `load_config` behind `handle_key` and make a
mixed-layer module read as a single colour, which is the one thing this
view exists to show. On the `mixed` fixture, `app` renders three colours
and `pure` renders one.

**The unclassified plane is drawn.** An adapter that emits a tier index
past the end of `tiers` is a blank page with an exception nobody sees, and
against a crate at 1.2% coverage that is most of the nodes. The tier list
now always carries a trailing `Unclassified` plane — reachable even at
100% coverage, because a module holding only `mod` declarations classifies
as nothing either. Drawing it is also right on its own terms: a symbol the
classifier missed is the one most worth looking at.

**Atlas's default map shows unclassified modules.** Its "platform" toggle
hides infrastructure; filtering the default view to *classified* modules
would have drawn an empty page for arioch and called it an atlas.

### `hotspots` availability

Being inside a git work tree is not enough — the fixture crates sit inside
this repo and have no commits of their own, so `git log` returns nothing
and the scatter draws empty. Availability now depends on actual churn.

### Narration

`fill()` consumes the `data-narr` attribute rather than keeping it, so a
`data-narr` surviving into the output always means a slot went unfilled —
which is a thing a test can assert, and one does. Slots with nothing to
say are removed element and all.

Seven slots, all filled from the model: `coverage`, `ports`,
`enforcement`, `unevaluable`, `layering`, `heuristics`, `cohesion`,
`baseline`. Against arioch the `ports` slot reads "No trait is defined
under domain/ports/, so this crate has no port seam" and the
`enforcement` slot reads "none is enforced by the compiler". Neither
sentence exists anywhere in a template.
