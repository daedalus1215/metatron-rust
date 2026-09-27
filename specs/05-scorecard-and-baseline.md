---
title: Scorecard, Baseline, and `cargo test`
status: implemented
project: metatron-rust
location: specs/05-scorecard-and-baseline.md
created: 2026-08-29
tags: [gating, baseline, ci, fitness-functions, cargo]
---

# Scorecard, Baseline, and `cargo test`

## Context

metatron-nestjs already solved the ratchet: fingerprint every violation, commit
a baseline beside the config, fail when a new fingerprint appears, report fixed
ones without silently retiring them. That design is sound and language-agnostic,
and spec 02 of the metatron-nestjs repo records both the reasoning and the bugs
found implementing it. It is ported, not redesigned.

Two things are new here.

The first is that **this project measures a climb, not a line held**. metatron
assumes a codebase that is mostly conformant and guards against drift. arioch is
at zero and moving toward the glossary. A ratchet that only says "no worse than
yesterday" is the wrong instrument for that; the number needs to move up and be
seen to move up.

The second is the reason for the whole rewrite: in Rust, the gate can be a test.

## Goal

Report conformance in a form that is honest about what was not evaluated, ratchet
violations so they cannot silently increase, and let `cargo test` be the
enforcement mechanism.

## Design

### The scorecard is a table, not a percentage

A single conformance percentage is the most requestable and least defensible
output this tool could produce. It averages rules of wildly different weight,
it moves for reasons nobody can reconstruct, and — fatally — it has to decide
what to do with unevaluable rules. Counting them as failures punishes a project
for not having ports yet; counting them as passes is a lie; excluding them
inflates the score of a codebase that has almost no architecture to grade.

So the headline is three counts and an exposure line:

```
metatron check · arioch

  coverage         0/312 symbols (0.0%)  !

  rules           20     upheld 2 · violated 6 · unevaluable 12
  enforcement            compiler 0 · metatron 15 · clippy 1 · advisory 5
                         ↳ no rule is enforced by anything but this tool

  violations       31    new 0 · known 31 · fixed 0

  cohesion          1 type over threshold
                    App  app.rs:31  9 components, 48 methods

  PASS — no new violations. 31 known.
```

The `enforcement` line is the one the glossary earns and no other tool prints.
It changes on its own as the project matures: do the `crate-graph.md` Option B
workspace split and three rules migrate from `metatron` to `compiler`, and the
arrow beneath it goes away.

`coverage` stays first, exactly as in metatron, and for the same stated reason:
a tool that quietly files half the code under "other" and then draws a confident
picture of it is worse than one that fails.

### Baseline

Ported from metatron with the format changed to TOML for consistency with
`metatron.toml` and `Cargo.toml`. `metatron.baseline.toml`, committed, sitting
**beside the config, not in `.metatron/`** — output is generated and gitignored,
and a ratchet that is not committed cannot hold a line.

```toml
version = 1
generated_at = "2026-08-29T10:00:00Z"
project = "arioch"

[violations.a3f19c4b2e01]
rule = "domain-no-io"
from = "application/tui/app.rs"
to   = "std::fs"
note = "pre-dates the Filesystem port; blocked on spec 04 of the arioch refactor"
```

Every detail carried over deliberately:

- **Fingerprint is `sha1(rule|from|to)[..12]`**, not a per-rule count. A count
  says "3 became 4"; only a fingerprint names the offender, and only a
  fingerprint catches a swap — one violation fixed and another introduced in the
  same change, which a count nets to zero and passes.
- **`note` is hand-written and never overwritten.** It is where the reason a
  violation is tolerated gets recorded, and it is what stops a baseline
  degrading into an unexamined list. `--update` preserves notes by fingerprint
  and reports any it had to drop.
- **Fixed violations are reported but never auto-removed.** A scan that
  temporarily fails to parse a file would otherwise quietly retire a real debt,
  which would return later as a "new" violation with no history.
- **Heuristic rules are never in the baseline.** Spec 04 marks five rules
  `kind: heuristic`; they carry `gate = false`, cannot enter the baseline, and
  cannot affect the exit code under any flag. `metatron baseline` prints which
  rules were excluded, so a rule cannot sit outside the gate unnoticed.

### Progress, not just regression

The one genuine addition to metatron's design. `--update` records the count at
each acceptance, so the baseline carries its own history:

```toml
[[progress]]
at = "2026-08-29"; violations = 31; coverage = 0.0
[[progress]]
at = "2026-09-15"; violations = 22; coverage = 41.2
```

`metatron check` prints the delta since the last entry. This costs almost
nothing and turns the file into the record of the refactor, which is what the
tool is actually for on this project. It is data only — nothing gates on the
trend, because a project is allowed to have a bad week.

### `cargo test` integration — the point of the rewrite

`crate-graph.md` Option A prescribes a hand-written source scan in
`tests/boundaries.rs`. That test is a worse version of this tool, maintained by
hand, in every project. Replace it:

```rust
// tests/architecture.rs
#[test]
fn architecture_holds() {
    metatron::check(".").assert_no_new_violations();
}

#[test]
fn app_stays_decomposed() {
    metatron::cohesion(".").assert_max_components("App", 3);
}
```

The library API is therefore the primary interface and the CLI is a wrapper over
it, not the other way around. Requirements that follow from being a test:

- **No network, no `.metatron/` writes, no stdout noise** on the library path.
  A test that mutates the working tree is a bad test.
- **Fast enough to run every time.** Target under 500 ms for a 10k-LOC crate;
  `syn` parsing dominates and is comfortably inside that.
- **Panic message is the report.** `assert_no_new_violations()` panics with the
  named violations, file and line, formatted as the CLI would print them —
  `cargo test` output is the only thing anyone will read.
- **`metatron` is a dev-dependency.** It must never appear in the dependency
  graph of a release build.

`assert_max_components` exists so the arioch refactor can lock in each
extraction as it lands. It is the ratchet applied to cohesion, which spec 02
declines to gate globally for good reason — but a threshold the author chose for
one named type, written in their own test file, is a different thing from a
threshold the tool imposes everywhere.

### Commands and exit codes

```bash
metatron scan [path]              # parse + classify, write model.json
metatron check [path]             # evaluate, compare to baseline, exit 0/1/2
metatron cohesion [path]          # spec 02 report
metatron baseline [--update]      # record today's violations as accepted
metatron views [path] [name]      # spec 06
```

| code | meaning |
|---|---|
| 0 | clean, or only known violations |
| 1 | new violations |
| 2 | scan or config error |

Distinct codes so CI can tell "the architecture regressed" from "the tool
broke" — metatron's reasoning, unchanged.

| flag | effect |
|---|---|
| `--rule <id>` | gate on named rules only; the rest advisory |
| `--allow-new <n>` | tolerate up to n new violations (default 0) |
| `--json` | machine-readable, for other tooling |
| `--no-fixed` | suppress the fixed-since-baseline section |

`--allow-new` exists so the gate can be adopted mid-refactor and ratcheted
toward zero. It is a number in CI config, so lowering it is a visible,
reviewable act.

## Out of scope

- Posting results anywhere. `check` writes stdout and sets an exit code.
- Per-rule severity thresholds beyond `--rule` filtering.
- A single conformance percentage. Declined above, on purpose.

## Acceptance

- `metatron baseline && metatron check` on an unchanged tree exits 0 with zero
  new and zero fixed.
- Introducing one `use rusqlite::Connection` in `domain/` exits 1 and names the
  exact file and line.
- Fixing a baselined violation is reported as fixed and does **not** mutate the
  baseline until `--update`.
- Hand-written `note` fields survive `--update`; dropped notes are reported
  separately from preserved ones, with an accurate count. *(metatron shipped a
  bug here — `--update` reported "1 note preserved" while preserving none. Test
  for it.)*
- No `heuristic` rule can enter the baseline or change the exit code, including
  under `--rule store-decides`.
- `metatron::check(".")` from a `#[test]` passes and fails correctly, writes
  nothing to disk, and produces a panic message naming the violations.
- Against arioch today: `check` exits 0 after `baseline`, and the scorecard
  shows `coverage 1.2%`, `unevaluable 18`, and `compiler 0`. A run that
  reports a healthy architecture for arioch is a failed acceptance test.
  *(The criterion said `0.0%` and `unevaluable 12`; both were estimates
  written before spec 03 and 04 existed. The test asserts the shape —
  coverage under 5%, unevaluable over 10, compiler exactly 0, upheld
  exactly 0 — rather than numbers that move when a rule is added.)*


---

## Implementation notes (2026-08-31)

`src/baseline.rs`, `src/scorecard.rs`, the test-facing API in `src/lib.rs`,
`metatron check` rewritten against the baseline, `metatron baseline
[--update]`. 13 tests in `tests/baseline.rs` and 4 in
`tests/architecture.rs` — the latter being metatron gating the conforming
fixture inside metatron's own suite, which is the thing this spec is for.
Added `sha1`. **74 tests total; a full `check` of arioch takes 76ms**,
comfortably inside the 500ms budget.

### Measured

```
metatron check · arioch

  coverage         1/83 symbols (1.2%)  !

  rules           22     upheld 0 · violated 3 · unevaluable 18 · pass 1
  enforcement            compiler 0 · metatron 15 · clippy 1 · advisory 6
                         ^ no rule is enforced by anything but this tool

  violations       3     new 3 · known 0 · fixed 0
  cohesion         1     App  app.rs:50  3 components, 46 methods  [Tangled]

  ADVISORY  4 finding(s) — reported, never gated
    renders-off-store   4  ui.rs:78  `render_sidebar` reaches `app.registry.entries`

  outside the ratchet
    port-has-fake       advisory — warns, does not fail
    panic-in-domain     delegated — guaranteed elsewhere
    store-decides       heuristic — cannot gate
    ... 4 more
```

### The layer rules were checking the wrong thing

Caught by the acceptance test for "introducing one `use rusqlite::Connection`
in `domain/` exits 1 and names the file and line". It did exit 1 — for the
wrong rule. `domain-no-io` never fired.

The cause: spec 04's rules are worded by **location** — "no symbol in
`domain/` has an `Extern` edge to an `io` crate" — and I had implemented
them against the spec 03 **classifier**. A symbol that matches no pattern
has no layer, so it is invisible to every layer rule.

That is exactly backwards. The unclassified symbol is the one most likely
to be doing something nobody named. The test function was called
`leak_activity`, which fails the use-case verb allowlist, so it classified
as nothing and its `rusqlite` import went unreported.

Layer rules now resolve a symbol's layer as *classified layer, else the
layer owning the directory it sits in*, with the directory prefixes
derived from the config's own `path` keys. Nothing is hardcoded, and a
project that renames `domain/` in `metatron.toml` gets the rule following
it. Pattern-specific rules — `call-through-port`, `no-same-level` — still
use classification, because those are about what a symbol *is*, not where
it lives.

### What the baseline holds, and what it refuses to hold

Only rules with `gate = true`. Heuristics are excluded by construction:
there is no flag, including `--rule store-decides`, that lets a guess fail
a build. `port-has-fake` warns rather than fails, so it is excluded too,
and `panic-in-domain` is delegated to clippy. Every exclusion prints its
reason under `outside the ratchet`, so a rule cannot sit outside the gate
unnoticed.

### The swap, tested

The argument for fingerprints over counts, made concrete on the leaky
fixture: rename `ActivityDto` (fixing one `dto-in-domain`) and add
`SessionDto` (introducing another) in the same edit.

```
before:  violations 26   new 0 · known 26 · fixed 0
after :  violations 26   new 1 · known 25 · fixed 1   -> exit 1
```

The total is identical. A per-rule count nets it to zero and passes.

### Notes, and the bug metatron-nestjs shipped

`--update` preserves notes by fingerprint and reports the dropped ones
separately, with the fingerprint and the note text so nothing is lost
silently:

```
  25 violation(s) accepted
  1 note(s) preserved
  1 note(s) dropped — their violation no longer exists:
    d9731aa15083  dto-in-domain   renamed next commit
```

There is a test asserting both counts, because the JS implementation
reported "1 note preserved" while preserving none. A known violation also
carries its note back into the report, so the reason a violation is
tolerated is visible on every run rather than only in the file.

### Test-safety, asserted rather than intended

`the_library_path_writes_nothing_to_disk` snapshots the fixture tree
before and after `metatron::check()` and `metatron::cohesion()` and
compares. A test that mutates the working tree is a bad test, and
"I was careful" is not a guarantee.

### Two assertions the spec did not have

- `assert_no_unevaluable()` — a green suite means nothing if half the
  rules had no premise. This is what notices when a refactor deletes the
  last port and eight rules quietly stop being checked.
- `assert_coverage_at_least(pct)` — the same argument for the classifier.

Both are in `tests/architecture.rs` against the conforming fixture.

### Progress

`--update` appends one `[[progress]]` entry per acceptance, at most one
per day, so re-running `--update` cannot manufacture a trend. `check`
prints the delta since the last entry:

```
  since 2026-08-31         violations +0 · coverage +0.7%
```

Data only. Nothing gates on the trend, because a project is allowed to
have a bad week.
