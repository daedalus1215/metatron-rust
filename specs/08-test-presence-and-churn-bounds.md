---
title: Test Presence and Churn Bounds
status: draft
project: metatron-rust
location: specs/08-test-presence-and-churn-bounds.md
created: 2026-09-27
tags: [tests, churn, gating, trust]
---

# Test Presence and Churn Bounds

## Context

Two numbers this tool already computes gate nothing.

`Symbol.is_test` is set on every symbol the scanner emits — `in_test` from the
enclosing module, `has_cfg_test` from the attribute, `is_test_fn` from `#[test]`
(`src/scan/mod.rs:489,535,561,680,777`). Four rules read it
(`src/rules.rs:628,824,887,1268`), cohesion reads it, five views read it, and
the classifier gives every test symbol the pattern `test`
(`src/classify.rs:418-425`). Every one of those readers uses it the same way: as
a subtraction. "`is_test` is set, therefore this symbol is not this rule's
problem." Not one of them reads it to ask the opposite question — whether a
symbol *has* a test. The word "test" in this repository means the tests of
metatron, not the tests *of* a symbol, and those are different questions.

Churn is richer. `src/views/hotspots.rs:71-130` shells out to
`git log --no-merges --numstat` and builds a per-file map: commits, lines added,
lines removed, first and last touch, distinct author count. There is no `--since`
and no window: the log is the whole history, and `ChurnMeta.since` is the
earliest date in it (`src/views/hotspots.rs:190-194`), which is the start of the
history rather than a bound on it. The combination that actually identifies a
hotspot — commits against dependents — is computed in the browser, at
`templates/hotspots.html:200` as `commits * (1 + deps)`, from a map the Rust
side never ranks. So the number that decides where refactoring pays for itself
exists in no report, gates nothing, and has never been in Rust at all.

The failure mode is the one this project's specs keep naming. A tool that
computes "is this tested" and "does this file keep changing" and then stays quiet
is not neutral; it is a tool that has declined to say the two things a reviewer
most wants said, and it has left the reasoning in a view where nobody's build
reads it. Worse, both numbers are easy to state falsely:

- **"Untested."** Not in the model. For a symbol with no test calling it, the
  model cannot distinguish "no test exists" from "no test *was scanned*". Today
  the second case is the default: `fn targets()` at `src/scan/mod.rs:134-200`
  resolves `src/lib.rs`, `src/main.rs`, `src/bin/*` and declared `[[bin]]`, and
  nothing else. Cargo's `[[test]]` targets and the auto-discovered `tests/*.rs`
  are not scanned at all. On a crate whose entire test suite is integration tests,
  this tool sees zero tests and would report every symbol as untested — a
  confident, total, wrong claim built from an absent file.
- **"Hot."** Churn is a function of the window and the age of the repository. A
  file with four commits in a repo created last week is not a hotspot; it is four
  commits. An absolute bound (`commits > 10`) is therefore a claim about a repo's
  history, not about a file's design, and it silently means something different in
  a two-year-old crate than in a two-week-old one. And when `git` is absent or the
  path has no commits, `churn()` returns an empty map and `ChurnMeta.available` is
  `false` — the same absence that should produce `Unevaluable` is instead a view
  with an empty scatter and a caption that still says churn. The view's own prose
  already knows this: `templates/hotspots.html:170` says a high score is not a
  verdict, and that commits are a proxy for churn rather than a measure of it.

## Goal

Make both numbers say what they mean, and let them hold a line.

- The model sees the crate's tests, including integration tests, or it names the
  ones it did not scan.
- A symbol with no test exercising it is reported as *not known to be exercised*,
  which is a statement about the model and is decidable. "Untested" is not
  claimed, because it is not knowable from a static scan.
- A rule whose premise is absent is `Unevaluable`. No test targets scanned means
  test presence is `Unevaluable`, not "everything fails". No git history means
  churn is `Unevaluable`, not "nothing is churning".
- Churn bounds are relative to the crate's own distribution and carry the window
  they were measured over, so the number means the same thing next month.
- Both rules state, in their own reason string, the weaker thing they actually
  established.

## Design

### The model's blind spot is the test suite

`targets()` gains the rest of what cargo would build: declared `[[test]]` paths,
and `tests/*.rs` plus `tests/*/main.rs` as cargo auto-discovers them. Test targets
are scanned into the same symbol table, tagged with the target name, so a symbol
knows which target it came from and a test knows it is one.

`Symbol.is_test` stays as it is — it is true for anything inside `#[cfg(test)]` or
annotated `#[test]` — and gains one honest companion: a symbol is a *test symbol*
when `is_test` is set. That is a fact about the source, not a claim about
coverage, and it is the only premise the rules below are allowed to use.

Scanning test targets has a cost that must be paid honestly: test code is test
code. Four rules subtract `is_test` today and the classifier hands test symbols
the pattern `test`, so a `test`-patterned symbol matches none of the layer names
the layering rules iterate (`use-case`, `service`, `store`, `command-handler` at
`src/rules.rs:772-790`, among others) and is quietly ignored. That is the
mechanism, and it is a mechanism rather than a guarantee: 22 rules, 4 of which
subtract `is_test`, is not a proof about the other 18. So the claim is tested
rather than argued — a fixture with a test target that imports across every layer
must produce exactly the findings it produced without the test target. If some
rule turns out to see test code as domain code, that is a finding about that rule
and it gets fixed here, not waved through because the pattern name looked
sufficient.

### Exercised, not tested

The only attribution available without a compiler is the call graph the model
already carries. A symbol is **exercised** when some test symbol has a `Call` edge
to it. Everything else is **not known to be exercised**.

`untested-port` is decidable on that premise and says exactly this:

```
no test in the scanned targets calls this port
```

Not "untested". Not "0% covered". The reason string is the contract, and the
finding carries the count of test targets that were scanned, because that count
is what makes the claim falsifiable: a reader who knows the suite is in
`tests/acceptance/` can see that those targets were not scanned and discount the
finding accordingly.

The tier is `advisory`, and this is a decision rather than a dodge. The premise
is decidable, but the *judgment* is weak — a test can exercise a port through its
caller, through a trait object, or by hand — and a gate that fires on every
symbol of a crate with no scanned tests is a gate that gets disabled. Making it
advisory means the number is available in every report, in the baseline, and in
`check --all`, without anyone having to believe it. Promoting it to a gate is a
one-line change to the tier table once a baseline exists, and the spec says so
rather than pretending the question is closed.

### Churn moves out of the view

`churn()` moves from `src/views/hotspots.rs` to a shared module that both the
view and the rules read, and its output is no longer an empty map when git is
missing. It becomes a struct that is either data or a stated absence:

```
Churn {
  files: BTreeMap<String, FileChurn>,
  window: { since, commits, files },
  available: bool,
  reason: String        // why not, when available is false
}
```

`reason` is not optional. "git is not installed", "`path` has no commits",
"the log covered 0 files" are three different absences and a reader who is told
only that churn is unavailable cannot tell whether the crate is untracked or the
tool is broken. This is spec 07's exit-2 instinct applied to a field that never
had an error channel.

The ranking moves with it. `commits * (1 + deps)` is computed in
`templates/hotspots.html:200` and must become a Rust function that both the view's
payload and the rule call, because a threshold and a scatter that disagree are two
different opinions about the same crate, and only one of them is in the report.

### A bound that means the same thing twice

Two premises before any churn rule fires:

1. **Minimum history.** Fewer than 20 commits in the window, and churn is
   `Unevaluable`, with the count in the reason. A repository cannot have hot files
   before it has a history, and a rule that reports a 30-commit-old crate's
   freshest file as its top hotspot is measuring the author's first week. The
   window is the whole history unless the tool is given one, and either way the
   reason says which.
2. **Relative position, not an absolute count.** The bound is on a file's share of
   the crate's total churn, and on its rank. A file in the top decile of churn
   *and* in the top decile of dependents is the hotspot; the same file in a crate
   where everything churns equally is not. Absolute thresholds would encode this
   project's idea of a busy file, which is not a fact about the scanned crate.

`churn-concentration` therefore reports:

```
this file holds 18% of the crate's churn and 6 of its 9 dependents — top decile
on both, over 214 commits since 2024-11-02
```

with the window in the string, because a bound without its window is a number
whose meaning expires silently. Tier: `advisory` for the same reason as above —
churn correlates with importance, and a build that fails on a file someone is
actively working on teaches the team to ignore the tool. It gates when it is
combined with a decision about who is allowed to change the threshold, which is a
policy question this repository has not answered.

### The two rules together

| rule | premise | decidable | tier | why |
|------|---------|-----------|------|-----|
| `untested-port` | a test symbol calls the port | yes | advisory | the premise is in the model, the judgment is not |
| `churn-concentration` | the crate has a history, and the file ranks in the top decile of churn and dependents | yes | advisory | correlates with being important |

Both are decidable, so neither may report `Unevaluable` for a reason that is not
stated in its reason string. Both are advisory, so neither can fail a build, which
is recorded in `tests/rules.rs` alongside the 16 existing gates — the same test
that pins enforcement status, extended so a new rule cannot arrive without a tier.

## Out of scope

- **Coverage percentages.** Line and branch coverage come from the compiler. This
  tool does not run tests, so every percentage it printed would be a fabrication.
- **Mutation testing, flakiness, test duration.** Same reason.
- **Inferring coverage through callers.** "No test calls this port, but 12 symbols
  do" is a real observation and belongs in the finding's context, not in its
  verdict. The reason string may state the dependent count; the rule may not
  excuse the finding on that basis, because the exception would be unfalsifiable.
- **Test targets as conformance subjects.** Test code is scanned and visible, and
  excluded from the layering rules. Making `tests/` a layer of its own is a
  classifier change, not a scanning one.
- **Git-log-derived authorship judgments.** `authorCount` is already in the
  payload. "Too many authors means too many opinions" is not a rule this project
  can defend, and it is not one.
- **Forcing either rule to gate.** Both tiers are recorded with a reason. Changing
  them is a deliberate edit to `rules.rs` and this spec, not a config tweak.

## Acceptance

- A crate whose tests are all in `tests/*.rs` has those files in the model, with
  `is_test` set on their symbols and the target named.
- A declared `[[test]]` with a `path` outside `tests/` is scanned; a declared
  `[[test]]` whose path does not exist is a `MissingTarget` diagnostic naming it.
- Test targets are excluded from the layering rules' premises by the mechanism
  described above, and a fixture that calls across every layer from a test target
  produces no new findings.
- `untested-port` reports the count of scanned test targets in its reason, and its
  reason never contains the words "untested" or "coverage" as a claim.
- With no test target scanned, `untested-port` is `Unevaluable` with a reason
  naming what was missing — not 40 findings.
- `churn-concentration` is `Unevaluable` when the window holds fewer than 20
  commits, and its reason says how many there were.
- `Churn.reason` is non-empty whenever `available` is false, and names which of
  git-missing, no-commits, and no-files-covered applied.
- The hotspots view's payload and the rules read churn from one implementation;
  the view's scatter is unchanged for a crate with history.
- No churn rule fires on an absolute commit count.
- Both new rules are listed in `tests/rules.rs` with their tier, and the
  `enforcement` partition still sums to the number of findings.
- `cargo fmt --check`, `cargo clippy --all-targets` and `cargo test` are clean, and
  no test is ignored to make it pass.
