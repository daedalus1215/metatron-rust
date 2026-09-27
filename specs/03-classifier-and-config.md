---
title: Classifier and `metatron.toml`
status: implemented
project: metatron-rust
location: specs/03-classifier-and-config.md
created: 2026-08-29
tags: [config, classifier, coverage, patterns-rust]
---

# Classifier and `metatron.toml`

## Context

metatron-nestjs classifies by **filename**: `.repository.ts` is a repository,
`.service.ts` is a service. Its own README names the limit — "suits projects
whose conventions live in filenames" — and Rust is not such a project. There is
no `.store.rs` convention and there should not be one.

`patterns-rust` carries its conventions in two places instead, and every pattern
page states both under *Where it lives* and *Rust idiom*:

- **module path** — `domain/ports/`, `domain/use_cases/`, `infra/`, `application/`
- **symbol shape** — `trait ActivityStore`, `SqliteActivityStore`, a verb-phrase
  `fn start_activity`, `*Params`, `*Projection`, `*Command`, `*Row` / `*Panel` / `*View`

Two signals is more information than metatron has ever had, not less. A filename
can only say one thing about a whole file; a path plus a symbol shape can say
different things about two symbols in the same file — which is what makes the
mixed-layer detector in spec 02 possible at all.

## Goal

Turn `patterns-rust` from prose into a config the scanner can execute, and
report honestly how much of the code it recognised.

## Design

### The config is declarative

metatron's `arch.config.js` is a JavaScript module holding `RegExp` literals and
a `moduleOf: (rel) => string` lambda. That does not survive the move to TOML,
and it does not need to: the glossary's classifier is a path prefix plus a
symbol shape, both of which are data. Losing the lambda makes the config more
inspectable, not less capable.

`metatron.toml`, beside `Cargo.toml`:

```toml
root  = "src"
name  = "arioch"            # defaults to the Cargo package name

# The intended call flow. Skip rules are derived from it (spec 04).
flow  = ["command-handler", "service", "use-case", "port", "store"]

# Layers, ordered as a call travels inward.
[[layer]]
id = "application"; title = "Application"; sub = "CLI + TUI entry points"
[[layer]]
id = "domain";      title = "Domain";      sub = "use-cases, ports, pure domain"
[[layer]]
id = "infrastructure"; title = "Infrastructure"; sub = "stores and adapters"
[[layer]]
id = "composition-root"; title = "Composition Root"; sub = "the only place concretes are named"
[[layer]]
id = "test"; title = "Test"; sub = "fakes and specs"

# Patterns. First match wins, so shape-specific rules precede path-only ones.
[[pattern]]
id = "port"; layer = "domain"
path = "domain/ports/"
symbol = { kind = "trait" }

[[pattern]]
id = "use-case"; layer = "domain"
path = "domain/use_cases/"
symbol = { kind = "fn", name = "^(start|stop|add|create|delete|update|list|find|scan|register|import|export|move|rename)_" }

[[pattern]]
id = "validator"; layer = "domain"
symbol = { kind = "fn", name = "^validate_", returns = "Result" }

[[pattern]]
id = "entity"; layer = "domain"; pure = true
path = "domain/"
symbol = { kind = "struct", has_field = "id" }

[[pattern]]
id = "value-object"; layer = "domain"; pure = true
path = "domain/"
symbol = { kind = ["struct", "enum"], derives = ["PartialEq"], not_has_field = "id" }

[[pattern]]
id = "store"; layer = "infrastructure"
path = "infra/"
symbol = { kind = "struct", implements_port = true }
externs = ["rusqlite", "sled", "toml", "serde_json"]

[[pattern]]
id = "adapter"; layer = "infrastructure"
path = "infra/"
symbol = { kind = "struct", implements_port = true }
externs = ["std::fs", "std::process", "reqwest", "arboard"]

[[pattern]]
id = "command-handler"; layer = "application"
path = "application/"
symbol = { kind = "fn", name = "^(cmd_|handle_)" }

[[pattern]]
id = "view-model"; layer = "application"
path = "application/"
symbol = { kind = ["struct", "enum"], name = "(Row|Panel|View|Command|Args)$" }

[[pattern]]
id = "composition-root"; layer = "composition-root"
path = "^(main\\.rs|application/tui/run\\.rs)$"

# Crates that mean "this symbol does I/O". Drives the domain-purity rules.
[externs]
io       = ["std::fs", "std::process", "std::net", "rusqlite", "reqwest"]
render   = ["ratatui", "crossterm"]
time     = ["std::time::SystemTime::now", "chrono::Local::now", "std::time::Instant::now"]

[naming]
forbid_in_domain = ["Dto", "DTO"]
use_case_params  = "Params$"
service_input    = "Command$"
domain_output    = "Projection$"
```

`extends = "patterns-rust"` supplies all of the above as a built-in default
profile, so a conforming project's config is four lines. `add_pattern` prepends
to the profile the way metatron's `addPatterns` does; `pattern` replaces it
wholesale.

### Store versus adapter is fuzzy, and the config says so

Both are "a struct in `infra/` implementing a port." The glossary distinguishes
them by *what they talk to* — `store.md` is persistence, `adapter.md` is "the
filesystem, an HTTP API, the clipboard, spawning `$EDITOR`". That is a real
distinction to a human and a soft one to a scanner, which is why the `externs`
key exists on both patterns above.

When a symbol matches both, or neither extern set, classify it as `infra-impl`
and record an `ambiguous-infra` diagnostic. Do not pick. Every rule in spec 04
that applies to a store also applies to an adapter, so nothing downstream
depends on getting this right — it affects the label in the views and nothing
else. Guessing would buy a nicer picture at the cost of a wrong one.

### Coverage is per symbol

metatron reports coverage over files. Here it must be over **symbols**, because
a file is not a unit: classifying `app.rs` as "application" while it contains
three domain value objects would report 100% coverage of a mixed-layer module.

```
coverage 0/312 symbols (0.0%)  !
  312 symbols matched no pattern. Add them to [[add_pattern]] in metatron.toml:
    struct in src/ root      41x   e.g. app.rs:31 App
    fn in src/ root         187x   e.g. app.rs:96 refresh_content
    ...
  0 ports found — 7 rules cannot be evaluated
```

The second line is the one that matters for the current state of both target
codebases. A conformance tool that reports "no violations" against a codebase it
classified nothing in is lying by omission. **Any rule whose premise does not
exist reports `unevaluable`, never `pass`.** Spec 05 carries this into the
scorecard.

### The flow drives the rules

Unchanged in spirit from metatron: declare

```toml
flow = ["command-handler", "service", "use-case", "port", "store"]
```

and the skip rules are derived — an edge that jumps one station is a warning,
two or more is critical. This is a direct transcription of the hierarchy in
`patterns-rust/dependency-hierarchy.md`, and it means nobody hand-lists rules.

Two Rust-specific additions to the derivation:

- **`port` is a permitted terminus.** An edge from `use-case` to `port` is the
  architecture working. An edge from `use-case` to `store` skips the port and is
  the defining violation — the flow already encodes this, which is the reason
  `port` appears in the list as a station rather than as an annotation.
- **`Impl` edges are exempt from skip rules and checked separately.**
  `impl ActivityStore for SqliteActivityStore` runs from `store` (infrastructure)
  backwards to `port` (domain). Against the flow that reads as a maximal
  violation; in hexagonal architecture it is the entire point. Spec 04 gives it
  its own rule.

## Commands

```bash
metatron scan [path]       # now also classifies; prints coverage
```

## Out of scope

- Rule evaluation (spec 04).
- Inferring a config from an unclassified codebase. Tempting, and wrong: a
  classifier that invents the architecture it then grades is circular. The
  coverage report tells the author what to write.

## Acceptance

- Against arioch as it stands: coverage is **0%** or near it, `impls` is empty,
  and the report names `App`, `Registry`, `Config` and the rest as unmatched
  with their file and line. Zero is the correct answer and must not be
  suppressed, rounded away, or reported as a pass.
- A hand-built fixture crate laid out per `patterns-rust` — `domain/ports/`,
  `domain/use_cases/`, `infra/`, `application/`, `main.rs` — classifies at
  100%, with `start_activity` as `use-case`, `ActivityStore` as `port`,
  `SqliteActivityStore` as `store`, `RealFs` as `adapter`, and `cmd_start` as
  `command-handler`.
- A struct in `infra/` implementing a port while naming neither a persistence
  nor an I/O crate is labelled `infra-impl` with an `ambiguous-infra`
  diagnostic, not guessed into one bucket.
- Two symbols in the same file classifying to different layers both keep their
  own classification; neither is overwritten by a file-level verdict.
- Removing `[[pattern]] port` from the config drops coverage and raises the
  "0 ports found" line — the tool's confidence tracks the config, not the code.


---

## Implementation notes (2026-08-31)

`src/classify.rs`, the profile at `src/profiles/patterns-rust.toml`,
`metatron classify [path] [--verbose] [--json]`, and a coverage line added
to `metatron scan`. 11 tests in `tests/classify.rs`, two new fixture
crates. Added `regex` as a dependency — the config's `name` keys are
genuine regexes and a substring matcher would have quietly mis-classified.

### Coverage, measured

| crate | classified | ports | verdict |
|---|---|---|---|
| `tests/fixtures/ports` | 13/13 (100%) | 3 | the glossary, laid out |
| `tests/fixtures/mixed` | 6/6 (100%) | 0 | shape-only rules, mid-refactor |
| `arioch` | **1/83 (1.2%)** | 0 | one `fn main` |
| `enoch` | **1/92 (1.1%)** | 0 | one `fn main` |

The denominator excludes modules (containers, not units) and methods
(which inherit their type — otherwise a 48-method god object would count
as 48 classified units and arioch's coverage would rise for being worse).

### The composition-root rule inflated arioch's coverage by 17x

The spec's config gives `composition-root` a path and no symbol shape:

```toml
path = "^(main\.rs|application/tui/run\.rs)$"
```

Which classifies **everything that happens to sit in `main.rs`**. In arioch
that is 17 symbols — `cmd_export`, `cmd_import`, `Cli`, `Command` and the
rest — and it reported arioch at **20.5%** coverage.

The glossary does not support that reading. `dependency-hierarchy.md`
defines the composition root as the code that "constructs concrete
adapters, binds them to the ports they satisfy, and hands them down" —
a role held by specific functions, not by a file. Constraining the rule to
`fn main` / `fn run` gives the honest 1.2%.

This is the same failure as spec 01's path walk-back and spec 02's dead
field, for the third time: **the bug made the codebase look better.** A
20% coverage figure would have gone into the spec 05 baseline as a
starting score, and every later reading would have been measured against
a number that was never real.

### `implements_port` needs two passes

A port is decided by path and kind alone. A store is "a struct in `infra/`
implementing a port", which cannot be evaluated until the ports are known.
So pass 1 classifies traits, pass 2 classifies everything against that set.

The test that matters here removes the `port` pattern from the config and
re-runs against the same model: coverage falls, `ports` drops to 0, and
`SqliteActivityStore` stops classifying as a store. The premise is gone,
not merely unproven — which is the behaviour spec 04 needs.

### Store versus adapter, not guessed

Both patterns carry `group = "infra-impl"`. When several patterns in one
group match, or none of their extern sets does, the label is the group id
and an `ambiguous-infra` diagnostic is recorded. In the fixture:

```
infra::sqlite::SqliteActivityStore   store        (rusqlite::Connection)
infra::fs::RealFs                    adapter      (std::fs::read_to_string)
infra::mem::MemStore                 infra-impl   ambiguous
infra::mem::FixedClock               infra-impl   ambiguous
```

The two fakes talk to nothing, so nothing decides them. The group
mechanism is general rather than hard-coded for infra, so a project can
declare its own pair of patterns a scanner cannot separate.

### Detector 2 lives in `classify`, not `cohesion`

Spec 02 sequenced the mixed-layer detector here because it needs a config.
`cohesion::analyse` takes only a model, so the detector landed on
`Classified::mixed_layer_modules`, and prints under `metatron classify`.

A finding while building its fixture: **under the patterns-rust profile
alone, a mixed-layer module is structurally impossible.** Every pattern is
path-anchored, and the layer paths (`domain/`, `infra/`, `application/`)
do not overlap, so two symbols in one module cannot land in two layers.
The detector can only fire on a project whose config classifies by symbol
shape — which is exactly the mid-refactor case it was written for, and
what `tests/fixtures/mixed` now is:

```
app    application + domain + infrastructure
```

`handle_key`, `enum Mode` and `load_config` in one file. That is
`arioch/src/app.rs`'s shape, reproduced small enough to assert on.

### What the report refuses to do

Against arioch it prints `1/83` and this:

```
0 ports found — no trait in `domain/ports/` anywhere in this crate.
  Every rule with a port in its premise is unevaluable, not passing.
```

Spec 05 carries that distinction into the scorecard. Nothing here infers a
config from the code: a classifier that invents the architecture it then
grades is circular, and the gap list is the to-do that replaces it.
