---
title: metatron-rust specs — index and sequencing
status: implemented
project: metatron-rust
location: specs/README.md
created: 2026-08-29
tags: [index, roadmap, rust, ddd]
---

# Specs

Eight specs, all written and implemented: 01-06 on 2026-08-29, 07 and 08 on
2026-09-27.
A Rust rewrite of [`metatron-nestjs`](../../metatron-nestjs) that checks Rust
codebases against `patterns-rust` — the layout convention the rules are written
against, named throughout but not a checkout beside this one. Numbered by
dependency, not by importance.

| # | spec | kind | depends on |
|---|------|------|-----------|
| 01 | [Symbol model and the `syn` scanner](01-symbol-model.md) | foundation | — |
| 02 | [Cohesion analysis (LCOM4)](02-cohesion-analysis.md) | new analysis | 01 |
| 03 | [Classifier and `metatron.toml`](03-classifier-and-config.md) | foundation | 01 |
| 04 | [Conformance rules and enforcement tiers](04-conformance-rules.md) | gating | 01, 03 |
| 05 | [Scorecard, baseline, and `cargo test`](05-scorecard-and-baseline.md) | gating | 04 |
| 06 | [Retargeting the views](06-views.md) | presentation | 03, 04 |
| 07 | [Trustworthiness and self-audit](07-trustworthiness-and-self-audit.md) | self-audit | 01-06 |
| 08 | [Test presence and churn bounds](08-test-presence-and-churn-bounds.md) | new analysis | 01, 04 |

## Why full Rust

The port is smaller than it looks. `templates/*.html` is 3,456 of metatron's
5,673 lines and does not port at all — the templates are data with a `__DATA__`
token, so `include_str!` embeds them in the binary and a string replace fills
them. `scan.js` (997 lines) is discarded either way, because `syn` replaces it.
What actually gets rewritten is ~864 lines of JS logic: `narrate`, `baseline`,
`violations`, `build`, `config`, and the six adapters.

The payoff is that the tool can be a `#[test]`. `patterns-rust/cross-cutting/crate-graph.md`
prescribes, for a single-crate project, a hand-written source scan:

```rust
// tests/boundaries.rs  (run in CI)
#[test]
fn domain_has_no_io() { /* walk src/domain, grep for rusqlite */ }
```

If metatron is a Rust library, that becomes:

```rust
#[test]
fn architecture_holds() {
    metatron::check(".").assert_no_new_violations();
}
```

The tool stops being an external linter someone remembers to run and becomes
part of `cargo test`. That is exactly the enforcement mechanism the glossary
asks for, and it is only available if the tool is Rust.

## Suggested order

**01 first, unconditionally.** Everything reads the model.

**Then 02, not 03.** This inverts the obvious order for a reason. Cohesion
analysis needs only the parser — no classification, no config, no conformance.
It therefore produces a real finding on the first run against code that follows
none of the glossary, which is the situation both target codebases are actually
in. On arioch's `App` (42 fields, 48 methods) it should name roughly eight
components and where they split. That is the highest-value output available
before any refactoring has happened, and it validates the scanner against a case
whose answer is already known by hand.

**Then 03, then 04.** Classification has to exist before rules can reference
layers. 04 is where the glossary becomes executable.

**Then 05.** Converts the tool from something that reports into something that
holds a line, and lands the `cargo test` integration that motivates the rewrite.

**06 last.** The views are the reason metatron is nice to look at and the reason
it is easy to over-invest in early. Against a codebase at 0% conformance, five
lenses render five pictures of the same undifferentiated blob. Numbers first.

## The state of the target codebases

Both are pre-refactor. This is the premise of the whole project, not an
oversight:

```
arioch    8 files   5,471 LOC   0 traits   0 tests   no domain/   App = 41 fields, 48 methods
enoch     5 files   3,577 LOC   0 traits   0 tests   no domain/   State = 21 fields, 30 methods
```

**Zero ports across both.** `patterns-rust/domain/port.md` calls the port "the
load-bearing pattern in Rust," and neither codebase has one. So metatron-rust is
not a mapper of an existing architecture — it is the instrument that measures a
refactor from 0% toward the glossary. The first honest run says so:

```
coverage 0/312 symbols (0.0%)  !
0 ports found — no port-dependent rule can be evaluated
```

That is the correct output, and specs 03 and 05 are written so it is reported
rather than papered over.

## arioch is a fixture, not a subject

The glossary already diagnoses arioch by hand, by file and line, on at least
four pages:

| page | finding |
|---|---|
| `domain/port.md` | `use crate::db::Db` in a use-case or TUI — cites `arioch app.rs:2` as "the defining violation" |
| `infrastructure/adapter.md` | `std::fs::read_to_string` and `$EDITOR` spawning inside `app.rs` — "the defining violation for arioch" |
| `design-philosophy.md` | `CONFIG_OVERRIDE`, a `static Mutex` hiding configuration |
| `cross-cutting/naming.md` | `registry.scan_with_config(paths, excludes, patterns)` mixes a capability with its arguments |
| `application/view-model.md` | `ui.rs` renders straight off `app.registry.entries`, coupling the painter to the TOML shape |
| `domain/entity.md` | names `Entry` as arioch's entity |

Every one of those is a test case with a known answer. The acceptance criteria
in specs 02 and 04 are written against them directly. A detector that cannot
find what its author already found by hand is not ready.

## The thread running through all seven

metatron's own specs README ends on this and it carries over intact: the failure
mode of an architecture tool is not crashing, it is drawing a confident picture
of something it did not understand. Rust makes this worse, not better — macros
generate items `syn` cannot see, `#[cfg]` gates code that may not compile, and
name resolution without a compiler is best-effort.

So each spec names its own refusal:

- **01** resolves paths best-effort and puts every unresolved one in
  `diagnostics` rather than dropping it or guessing.
- **03** reports symbol-level coverage, so a config that classifies nothing
  cannot masquerade as a clean architecture.
- **04** splits every rule by whether it is *decidable* or *heuristic*, and only
  decidable rules may gate a build.
- **02** refuses to suggest a split for a type it could not fully parse.
- **07** refuses to report a scan of one file as a census of the crate, and
  refuses to print an unqualified `PASS` over a scan that examined almost
  nothing.
