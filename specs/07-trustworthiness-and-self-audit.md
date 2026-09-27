---
title: Trustworthiness and self-audit
status: implemented
project: metatron-rust
location: specs/07-trustworthiness-and-self-audit.md
created: 2026-09-26
tags: [trustworthiness, diagnostics, coverage, exit-codes, ci, self-audit]
---

# Trustworthiness and self-audit

## Context

`specs/README.md:120-136` ends on this, and it is the only sentence in six specs
that matters more than the rest:

> the failure mode of an architecture tool is not crashing, it is drawing a
> confident picture of something it did not understand.

Specs 01 through 06 are written against that sentence, and the hard parts hold.
The fingerprint is `sha1(rule|from|to)[..12]` (`src/baseline.rs:70`) and the swap
it exists to catch is tested (`tests/baseline.rs:94`). Heuristic rules cannot
enter the baseline under any flag. Twenty-two rules split seventeen decidable
from five heuristic; sixteen gate, and the one decidable rule that does not is
`port-has-fake`, which is a convention a team adopts over a refactor rather
than a property of a build. `CfgExcluded` exists in the
model. A fixture crate of deliberately tangled Rust is the corpus, not a mock.

But the discipline stopped being applied at the edges, and at the edges is where
it was never optional. The audit that prompted this spec found four places where
metatron-rust reports a number it did not earn.

**It cannot see most crates.** `src/scan/mod.rs:80-84`:

```rust
let entry = ["main.rs", "lib.rs"]
    .iter()
    .map(|f| self.root.join(f))
    .find(|p| p.exists())
```

First match wins. On a crate with both targets — the ordinary layout for anything
with a library and a binary, which is most crates — it picks `main.rs` and never
opens `lib.rs`. Nothing reports this. The scan succeeds:

```
$ metatron scan --stdout .        # on metatron-rust itself
  1 files · 1 modules · 699 loc · 9 symbols
```

Fifteen files, four hundred symbols, `lib.rs`, `rules.rs`, `views/`, `scan/` —
all absent, and the summary reads like a complete census. It is not. It is worse
than a failure, because `check` will now enforce an architecture over one file.
The sixteen `UnresolvedPath` diagnostics that same run emits are the *symptom*:
`lib.rs`'s `crate::` paths are absent from the symbol table, so the crate-root
edge at `src/scan/mod.rs:727-728` cannot resolve and interpolates an empty module
name, printing `` `metatron::scan` in `` — a detail string that ends in a
preposition with nothing after it.

**It cannot be told apart from a clean bill of health.** `src/main.rs:91-94`
states the intent exactly right: *"Exit 2 distinguishes 'the tool broke' from
'the architecture regressed' — CI needs to tell them apart."* Then `scan`,
`classify` and `cohesion` return their `Result` straight out of `main`, and Rust's
`Termination` gives them exit **1** — the code `check` uses for a regression.

```
scan /nonexistent -> 1    classify -> 1    cohesion -> 1
check /nonexistent -> 2   views -> 2       baseline -> 2
```

Half the CLI cannot be trusted in a pipeline for the reason the comment was
written to prevent.

**It reports a green check on a blind scan.** `assert_coverage_at_least` and
`assert_no_unevaluable` exist at `src/scorecard.rs:220,234` and are wired to
`tests/architecture.rs` — but no CLI flag calls them, and the scorecard's own
numbers are printed without consequence:

```
$ metatron check .                # on metatron-rust itself
  coverage 1/9 symbols (11.1%)  !     unevaluable 19 · pass 3
  PASS — no new violations.  exit=0
```

Nineteen of twenty-two rules had no premise, one symbol in nine was classified,
and the verdict is `PASS`. The gates exist. The door is not wired to them.

**It can emit a broken page.** `src/views/mod.rs:225` writes the serialized
payload into the template's `<script>` tag with nothing between the two. A
`</script>` anywhere in a rule message or path terminates the element and the rest
of the payload renders as text. metatron-nestjs hard-fails on this at
`src/build.js:134`.

Three smaller ones, each the same shape:

- `crossDomain` is hardcoded `vec![]` at `src/views/atlas.rs:191` and shipped to
  the browser, where `templates/atlas.html:283,530,534` renders the count and a
  table from it. The atlas view states **0 dependencies cross a bounded
  context** about a crate it has not checked. The repository already holds the
  right principle and the test that encodes it — `schema_is_absent_rather_than_
  present_and_empty`, `tests/views.rs:59` — for the `schema` view. The atlas
  breaks it.
- `CfgExcluded` is declared at `src/model.rs:154` and constructed nowhere.
  Spec 01:166-170 promised it: cfg-gated code is *"tagged; a `CfgExcluded`
  diagnostic notes that some of the model may not compile."* Every `#[cfg]`
  symbol is scanned as if it were live, unremarked.
- `panic-in-domain` is `Status::Delegated` with the reason
  `"clippy::unwrap_used / expect_used, denied per-module in clippy.toml"`
  (`src/rules.rs:958-962`). There is no `clippy.toml` in this repository, and
  clippy's `unwrap_used` is a crate-wide lint that cannot be denied per module.
  The rule delegates to a mechanism that does not exist, in a project that holds
  six panics of its own.

And the spec set — the strongest asset either repository has — has drifted from
what the code does. `specs/01:178-181` claims the model carries `coverage`,
`findings`, `violations` and `churn`; it carries none of them, they are computed
in three other modules and never serialized. `specs/04` renders the same run three
different ways across `:46`, `:223` and its own measured block. `specs/README.md:3`
still reads `status: draft` for the index. `specs/02:205` defers coupling to
*"metatron's logical-coupling spec 03"*, which does not exist in this repository.
`README.md:4` links `../../patterns-rust/README.md`, which resolves to nothing.
`tools/README.md:1` is titled `# n`, and both it and `specs/06:13` point at
`tools/n.mjs` — the file is `render-check.mjs`. A rename left the docs behind,
which is the exact failure mode the docs exist to prevent.

None of this is a redesign. Every fix is a decision this project has already made
in writing and then not carried out at the boundary.

## Goal

Make every number metatron prints one it can defend, and make it impossible for a
caller to mistake "I did not look" for "there is nothing there."

## Design

### Every target, or a diagnostic naming the ones that were skipped

Entry-point discovery becomes exhaustive instead of first-match, and a crate
layout the tool cannot cover is reported rather than quietly narrowed.

| target | how it is found |
|---|---|
| library | `src/lib.rs` |
| default binary | `src/main.rs` |
| auto-discovered binary | `src/bin/*.rs`, `src/bin/<name>/main.rs` |
| declared binary | `[[bin]] path`, which may point outside `src/` |
| example, bench | **not scanned** — a `TargetSkipped` diagnostic naming each |
| workspace members | `[workspace] members` — a diagnostic, see below |

All present library and binary targets are walked. Both `src/lib.rs` and
`src/main.rs` are two compilation units and are scanned as two; the file is not
chosen, the file is enumerated. `stats` gains a per-target breakdown so a
dual-target scan says so.

Examples and benches are the one deliberate exclusion. Cargo compiles them, so
they are arguably part of the crate, but they are not the architecture and
folding `examples/` into the model would put demonstration code in a diagram of
the product. A decision not to look is still a decision, so each one is a
diagnostic rather than a silence.

The module tree needs distinct ids for the two roots — `(crate)` is already
taken by the first entry walked, and a second node with that id is a collision
that would merge two crates into one node. `(crate)` and `(bin:main)` is the
intent; the exact scheme is an implementation choice, the requirement is that the
ids are distinct and that both files appear.

Two cases resolve to a diagnostic rather than a scan, because a narrower scan
dressed as a whole one is the thing this spec exists to stop:

- **A declared `path` that does not exist.** `[[bin]] path = "tools/cli.rs"`
  with the file deleted is a broken manifest, and the tool says which target it
  could not open. It does not report a scan of the remaining targets as if it
  were the crate.
- **A workspace member.** Resolving the full workspace graph is a dependency
  this project declined, so metatron scans one crate and names the rest. Which
  of two shapes it is depends on whether the directory has a package of its
  own:
  - a **virtual manifest** — `[workspace]` and no `[package]` — has no source at
    all. That is an error, not an empty model: a model with zero symbols named
    `unknown` is a worse answer than a sentence naming the members and saying
    which directory to point at instead.
  - a **package that is also a workspace root** has its own source, so it is
    scanned normally and the members beside it become one `TargetSkipped`
    diagnostic. Each member is a separate crate with its own architecture.

`read_manifest` (`src/scan/mod.rs:1099`) already parses `Cargo.toml` into a
`toml::Value` and returns the package name and dependency set. Entry points come
out of the same parse.

### Exit 2 means the tool broke, in every subcommand

`main` stops returning `Result` and returns `ExitCode`, with one place that maps
a `Result` onto it. The table in spec 05:177-181 becomes a property of the binary
rather than an intention in a comment:

| code | meaning |
|---|---|
| 0 | clean, or only known violations |
| 1 | the architecture regressed, or a requested gate was not met |
| 2 | scan, config, or render error |

Every arm returns through the same handler. The regression signal is a `Result`
from `check` that is genuinely a finding, not a `?` that escapes `main`.

The three-way split becomes testable, which is the actual deliverable. `cargo`
exposes the built binary to integration tests as `CARGO_BIN_EXE_metatron`, so
`tests/cli.rs` can assert exit codes against the real process — the thing the
table is about — rather than inferring them from a function's return type.

### A green check is never unqualified

Three mechanisms, in increasing order of force. The first two are always on; the
third is opt-in, because a project at 0% conformance is the situation this tool
exists for and must not be unable to run.

**The caveat is unconditional.** When a majority of rules are unevaluable, `check`
does not print `PASS — no new violations.` It prints the pass and then says what
was not examined:

```
  PASS — no new violations. 0 known.
  ↳ but 19 of 22 rules had no premise, and 1 of 9 symbols was classified.
    A green check over a blind scan is not a result. Say so, or fix the config.
```

`PASS` is a claim about the architecture. It is only allowed to stand unqualified
when the tool can say what it looked at.

**`--min-coverage <pct>`** fails the gate when symbol coverage is below the
threshold, names the measured value, and exits 1. It is the CLI face of
`assert_coverage_at_least`.

**`--require-evaluable`** fails when any rule lost its premise, names which rules
and why each has none, and exits 1. A refactor that deletes the last port and
quietly stops eight rules from being checked is a regression, and the gate should
say so.

Neither flag is on by default. A project mid-climb from zero must be able to run
`check` and get an honest report rather than a wall.

### The payload cannot break the page

Two sequences terminate a `<script>` element: `</script` and `<!--`. Both are
escaped in the serialized payload before substitution — `</` to `<\/`, which is a
legal escape for `/` inside a JSON string and therefore keeps the payload
byte-identical to the document a JSON parser would produce.

This is preferred over metatron-nestjs's hard failure at `src/build.js:134`
because the inputs are ours: a rule message and a file path are both strings this
tool composes, and refusing to render because one of them contains four
characters is a worse answer than rendering it correctly. The test is that the
page still parses, not that the tool declines.

### Say what you did not understand

**`CfgExcluded` gets constructed.** One diagnostic per file that contains any
cfg-gated item, naming the file and the count — not one per symbol, which is
noise that trains the reader to skip the section. The symbols carry their raw
predicate at `src/scan/types.rs:52-53` and keep doing so; `#[cfg]` is recorded,
not evaluated, and that remains true. What changes is that the model admits the
part of it which may not compile.

**Unmatched symbols get named by default.** metatron prints a coverage number and
stops; metatron-nestjs prints the number and then the exact files, grouped by
suffix, with the config key to add (`bin/metatron.js:249-260`). The Rust
equivalent groups unmatched symbols by the file they sit in, prints the groups
with counts and a worked example, and names `metatron.toml` as the place to fix
it — whenever unmatched exceeds 10%, so a nearly-clean crate is not scolded for
three stragglers. `metatron classify --verbose` already lists them individually;
this is the same information at the threshold, on the default path, for `scan`
and `check` as well as `classify`.

**No diagnostic detail is truncated or empty.** The `` `metatron::scan` in `` at
`src/scan/mod.rs:727-728` is a symptom of the scanner, not a formatting bug, and
the entry-point fix removes it. The invariant is worth stating anyway, because it
is the kind of thing that survives a refactor: a detail string that ends in a
preposition has lost the thing the preposition referred to.

### One predicate answers "does this rule gate?"

Three sites derive it three ways. `src/rules.rs:316` filters
`f.gate && f.status != Status::Delegated`; `src/baseline.rs:293` filters `f.gate`;
`src/scorecard.rs:107` buckets on `Tier::Clippy`. They agree today. Nothing makes
them agree tomorrow, and the failure is invisible: a rule that gates in the
baseline and is reported as advisory in the scorecard is a gate that does not
gate.

`Finding::gates()` becomes the single expression, defined once next to the rule
table, and all three call it. The deliverable is a test that fails when a rule is
added without choosing its enforcement status — `all_rules_have_an_enforcement_
status` asserting that every rule in the table is assigned one of the four
`Status` variants, so a new rule cannot inherit a default.

`panic-in-domain` is the live case. It is either enforced here, with a decidable
premise — `syn` can see `.unwrap()`, `.expect()` and `panic!` in a domain symbol
as easily as it can see a `use` statement, which would move the exposure line
from `metatron 15 · clippy 1` to `metatron 16 · clippy 0` — or its reason names a
mechanism that exists. A reason string citing a file the repository does not
contain is a failed acceptance test, and the cheapest honest fix is to correct
the string; the better one is to implement the check, since decidable is the
whole axis spec 04 is built on.

### Nothing permanently empty reaches the browser

`cross_domain` is removed from the atlas payload, and `templates/atlas.html`
stops reading `D.crossDomain` at `:283`, `:530` and `:534`. The generalisation is
one test: **no view payload serialises a key that is empty for every fixture.**
A lens that reports zero for a thing nobody computed is the atlas's version of
the coverage problem, and it is caught by the same assertion.

### The tool passes its own hygiene

- `cargo clippy --all-targets` is clean. Eleven warnings, all mechanical:
  `map_or` at `src/cohesion.rs:321,502`, identical `if` blocks at
  `src/scan/body.rs:124`, a manual `Iterator::find` at `src/scan/mod.rs:794`,
  `&PathBuf`→`&Path` at `src/main.rs:390,614,667`, and two in `tests/scan.rs`.
- `tools/render-check.mjs` is reachable from a documented one-line command with
  a `tools/package.json` beside it, and every name in `tools/README.md` and
  `specs/06:13` matches a file that exists.
- Every relative link in `README.md` and `specs/*.md` resolves, and the sample
  `scan` output at `README.md:36-47` is the output of a real run — currently
  missing the coverage line, which is the most important number in it.
- The stale numbers in the spec set are corrected where the code is right and
  annotated where the spec is: spec 01's "minimum adaptation" claim about the
  model's contents, spec 04's three renderings of one run, `specs/README.md:3`'s
  `status: draft`, and `specs/02:205`'s reference to a spec in another
  repository.

## Out of scope

- **Gating on coverage or unevaluability by default.** arioch is at 1.2% and both
  target codebases are pre-refactor by design (`specs/README.md:79-100`). A tool
  that cannot run on the situation it was built for is not stricter, it is
  unusable. The flags exist; they are opt-in.
- **Evaluating `#[cfg]`.** Recorded, and now announced. Spec 01:218-225.
- **Type inference, trait-object dispatch, macro expansion.** Spec 01:218-225.
- **`cargo_metadata` and real workspace resolution.** The diagnostic in
  "Every target" is the honest substitute at the cost of one dependency. If
  workspace support is wanted properly, it is its own spec with its own refusals.
- **Wiring `render-check.mjs` into `cargo test`.** `jsdom` and `node-canvas` are
  heavy native dependencies, and `node-canvas` does not load in this environment
  at all (`libcairo.so.2` is not page-aligned). A check that cannot run is not a
  check; the Rust-checkable half is already in `tests/views.rs`.
- **A CI workflow.** No `.github/` exists and this spec does not create one. The
  exit-code table in "Exit 2 means the tool broke" is the thing a workflow would
  consume, and it is now testable without one.
- **Any new analysis.** Specs 08 (test presence and churn bounds), 09 (logical
  coupling) and 10 (blast radius) are unaffected by this spec except where noted.

## Acceptance

- A fixture crate with both `src/lib.rs` and `src/main.rs` reports every symbol in
  both, and emits no `UnresolvedPath` for the crate root. *(The test that would
  have caught the bug: `tests/scan.rs` has no dual-target case, which is why this
  survived six specs.)*
- `metatron scan --stdout .` on metatron-rust itself reports every file under
  `src/`. The 1-file result is the regression this criterion is written against.
- A `src/bin/foo.rs` file is scanned, and a `src/bin/foo/main.rs` is scanned.
- A `[[bin]] path` pointing outside `src/` is scanned, and its file is recorded
  crate-relative — no absolute path from the machine that ran the scan reaches
  the model.
- A declared path that does not exist produces a `TargetSkipped` diagnostic
  naming that target, and the summary does not present the remaining scan as the
  whole crate.
- A declared `[[example]]` produces a `TargetSkipped` diagnostic. Not scanning
  it is a decision, and a decision gets said out loud.
- Pointed at a workspace root that is also a package, metatron scans that
  package, does not cross into `inner`, and names `inner` in a
  `TargetSkipped` diagnostic.
- Pointed at a virtual manifest, metatron fails with an error naming the
  members and the directories to point at instead. It does not return a model
  with zero symbols.
- A dual-target scan reports its targets separately in `stats`, and the two module
  tree roots have distinct ids.
- Every subcommand exits 2 on a scan or config error — all six asserted, not
  spot-checked, in `tests/cli.rs` against `CARGO_BIN_EXE_metatron`. There are
  six subcommands; "seven" counted `help`.
- `metatron check` exits 0 clean, 1 on a new violation, and 2 on a tool error,
  asserted as the same three-way split.
- `metatron check` never prints a bare `PASS` when most rules are unevaluable; it
  names the counts and says a green check over a blind scan is not a result.
- `metatron check --min-coverage 50` fails the gate below the threshold, names
  the measured coverage, and exits 1.
- `metatron check --require-evaluable` fails when a rule lost its premise, names
  which rules and why each has none, and exits 1.
- Both flags pass on a crate that meets them, so they are not vacuous.
- A view payload containing a literal `</script>` renders, and the page's JSON
  still parses to the same document.
- Every file holding a cfg-gated item produces exactly one `CfgExcluded`
  diagnostic naming the file and the count — not one per symbol.
- A crate with more than 10% unmatched symbols has `scan` and `check` name the
  groups and point at `metatron.toml`, with no `--verbose`.
- No `Diagnostic.detail` is empty, and none ends in a dangling preposition.
- `panic-in-domain` is enforced by this tool with a decidable premise, or its
  reason names a mechanism that exists. A reason citing a file the repository
  does not contain fails this criterion.
- Adding a rule without choosing its enforcement status fails a test.
- No view payload serialises a key that is empty for every fixture; the atlas
  payload no longer carries `crossDomain` and its template no longer reads it.
- `cargo clippy --all-targets` is clean.
- `tools/render-check.mjs` runs from the one-line command its README documents,
  and every filename referenced in `tools/README.md`, `specs/06-views.md` and
  `README.md` exists.
- Every relative link in `README.md` and `specs/*.md` resolves, and the sample
  `scan` output in `README.md` is copied from a real run of the current binary.
- The scorecard's `enforcement` line, the baseline's exclusion list, and
  `outside the ratchet` are derived from one predicate, and a test asserts the
  three agree for every rule.
- The whole suite still passes, and no test is ignored to make it pass.
