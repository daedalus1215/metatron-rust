---
title: Test Presence and Churn Bounds
status: implemented
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
code. The classifier already handles it — a symbol with `is_test` set is given
the pattern `test` and the layer `test` (`src/classify.rs:418-426`), and `test`
is a real layer in the profile it comes from: *"Test — fakes and specs"*,
`src/profiles/patterns-rust.toml:30-32`. So the invariant to hold is not "test
code is invisible" but the sharper one: **a symbol in a test target is in the
`test` layer and in no other**. A test classified as `domain` is a layering bug
that every rule downstream would act on.

Four rules subtract `is_test` as well (`src/rules.rs:628,824,887,1268`), and
cohesion and five views read it. None of that is a proof about the other 18 rules
out of 22, so the claim is tested rather than argued, and the test is
mutation-checked: the fixture's `tests/acceptance.rs` defines `fn helper()`,
which the fixture's own patterns *would* classify as `domain`, so deleting the
classifier's test-target branch makes the guard fail rather than quietly
reclassifying test code. The verdict map is also compared with and without
`tests/` and must be identical.

### Exercised, not tested

The only attribution available without a compiler is the call graph the model
already carries. A symbol is **exercised** when some test symbol has a `Call` edge
to it — or when a test *implements* it. The second half was not in the first draft
of this spec, and the fixture caught it: the ordinary way a suite touches a port
is by faking it, and an `impl` block is not a call. A premise that counted only
calls reported every port of a crate whose suite is entirely fakes as untested,
which is the confident wrong answer this project keeps refusing to print.
`ImplBinding` already carries `is_test` and the resolved `trait_id`, so this is a
read rather than a resolution of its own.

Everything else is **not known to be exercised**.

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
advisory means the number is in every report and in `check --all`, without anyone
having to believe it. It is *not* in the baseline: `baseline::current` excludes
non-gating rules on purpose, so an advisory finding can never be accepted and
never has to be defended. It is a standing observation, not a ratcheted one, which
is why it cannot be a gate yet and why promoting it later means a baseline that
was never built. That is the honest cost of this decision, and the reason the
rule ships warning rather than trusted.

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

`reason` is not optional. "git is not installed", "this repository has no
commits", and "no commit touches `src`" are three different absences, and a
reader told only that churn is unavailable cannot tell whether the crate is
untracked or the tool is broken. The middle case is worth noting: a repository
with no commits makes `git log` *fail* rather than return an empty log, so the
reason in that case is git's own sentence rather than a phrase invented here.
That is the better answer — it is the tool that knows, and its wording is
specific. This is spec 07's exit-2 instinct applied to a field that never had an
error channel.

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
| `untested-port` | a test symbol calls or implements the port | yes | advisory | the premise is in the model, the judgment is not |
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
- Every symbol in a test target is classified to the `test` layer and to no other
  layer, and the guard is mutation-checked: disabling the classifier's test-target
  branch fails it.
- The verdict map is identical with and without the test target present.
- `untested-port` reports the count of scanned test targets in its reason, and its
  reason never contains the words "untested" or "coverage" as a claim.
- A test that *implements* a port satisfies the rule. A fixture whose suite is
  entirely fakes and that calls nothing reports no ports, and one port left
  unfaked is named.
- With no test target scanned, `untested-port` is `Unevaluable` with a reason
  naming what was missing — not 40 findings.
- `churn-concentration` is `Unevaluable` when the window holds fewer than 20
  commits, and its reason says how many there were.
- A crate that is not a git repository leaves `churn-concentration`
  `Unevaluable` with git's own reason. Consequently `--require-evaluable` fails
  on any directory that is not a repository, which is correct and surprising
  enough to be worth stating: a fixture sweep cannot evaluate this rule, so the
  architecture test exempts it by name and `tests/churn.rs` evaluates it against
  this repository instead.
- `Churn.reason` is non-empty whenever `available` is false, and names which of
  git-missing, no-commits, and no-files-covered applied.
- The hotspots view's payload and the rules read churn from one implementation;
  the view's scatter is unchanged for a crate with history.
- No churn rule fires on an absolute commit count.
- Both new rules are listed in `tests/rules.rs` with their tier, and the
  `enforcement` partition still sums to the number of findings.
- `cargo fmt --check`, `cargo clippy --all-targets` and `cargo test` are clean, and
  no test is ignored to make it pass.

## Implementation notes (2026-09-27)

Four commits, in the order the dependencies forced: the spec, then the model's
blind spot, then the measurement, then the two rules.

### Test targets are scanned

`targets()` gained the rest of what cargo would build — declared `[[test]]` paths
and the auto-discovered `tests/*.rs` and `tests/*/main.rs` — and `Target` gained a
kind, so a test target is a third thing rather than a second binary. Every symbol
in a test target is `is_test`, seeded at the target rather than inferred from
`#[test]`: the target is the premise. `Model.stats.targets` reports them with
`kind: "test"`, which is the count `untested-port` puts in its reason.

A declared `[[test]]` whose path does not exist is a `TargetSkipped` diagnostic,
on the same terms as a missing `[[bin]]`. Cargo would fail to build that crate;
a scanner that said nothing would be the only thing in the toolchain that had not
noticed.

The `ports` fixture grew a `tests/clock.rs`, which is what gave the conforming
crate a suite to have a premise about — and, as a side effect, two extra
inversions, because a test fake *is* an inversion. `the_inversion_arrow_is_
counted_as_the_architecture_working` now asserts the split (five in production,
two in `tests/`) rather than a single 7, so a reader can see what the rule counts
without re-deriving it.

### The guard on test code, and how weak it first was

The invariant is that a symbol in a test target is classified to the `test`
layer and to no other. `test` is a real layer in `patterns-rust` — *"fakes and
specs"*, `src/profiles/patterns-rust.toml:30-32` — so the guarantee is not
"invisible" but "in `test` and nowhere else".

The first version of this guard asserted that no test-target symbol was
classified to *any* configured layer, which was wrong in a way worth recording:
`test` is configured, so the assertion failed immediately. The second version
compared the whole verdict map with and without `tests/`, which passed — and
passed under mutation too. Removing the `is_test` flag from test targets entirely
did not change a single verdict, because a `test`-layered symbol is invisible to
every layering rule whether or not the flag is set. A guard that cannot fail is
not a guard, so the fixture now defines `fn helper()` inside `tests/`, a name its
own patterns would classify as `domain`: deleting the classifier's test-target
branch now fails the test. That is the difference between an argument and a check.

### Churn moved, and stopped being an empty map

`src/churn.rs` owns the measurement. Three things changed rather than moved:

- **The formula left the browser.** `commits * (1 + deps)` was JavaScript in
  `templates/hotspots.html` and is now `churn::score`, called from Rust, with
  `deps` and `score` in the payload. The template reads both instead of counting
  fan-in itself. A first attempt kept the score as both a field and a method and
  serialised the field, so every file reported `score: 0` while the chart drew
  the old computation — caught by reading the rendered payload rather than by a
  test, which is why `every_file_reports_the_score_the_formula_gives` exists now.
- **Absence has a channel.** `Churn { available, reason }`, and `reason` is
  non-empty whenever `available` is false. A repository with no commits makes
  `git log` *fail* rather than return an empty log, so that case carries git's
  own sentence rather than a phrase invented here.
- **The window is stated.** There is no `--since`; the log is the whole history
  and `since` is where it starts. The stat row now says so, and every
  `churn-concentration` instance carries `over N commit(s) since DATE`.

`deps` is counted from `Model::file_links`, the same definition of "imports" the
rest of the tool uses, and `the_dependents_are_the_edges_the_model_already_has`
recounts it independently so a change to that definition cannot quietly change
the number and its check together.

### The two rules, and what they are not

`untested-port`'s premise had to become "calls **or** implements" after the
fixture demonstrated the narrow version was wrong — a suite of pure fakes calls
nothing and exercises everything. That is the single most important correction in
this spec, and it was found by a fixture rather than by reading.

Both rules are `advisory`, pinned in `tests/rules.rs` beside the six that were
already warnings, with the reasoning in the test. Neither gates: a gate that
fires on every port of a crate whose suite this tool did not scan, and hardest on
the file someone is actively working on, is a gate that gets switched off.

`churn-concentration` cannot be evaluated on a fixture at all, because a fixture
directory is not a repository. `assert_no_unevaluable` grew an
`assert_no_unevaluable_except` for it — an exemption with a named owner rather
than a deleted guard, and
`tests/churn.rs::the_churn_rule_is_decidable_against_a_real_repository` is the
repayment: the rule is decidable, is evaluated against this repository, and every
instance it produces carries its window.

That exemption has a consequence worth writing down: **`--require-evaluable` now
fails on any directory that is not a git repository**, with
`churn-concentration` named. Correct, and surprising enough that the two CLI tests
for the flag now run against a temporary repository with 25 real commits under
`src/` rather than against a fixture.

### One false number, found by running it

`Window.commits` was the sum of the per-file commit counts, which is file
touches. A scratch repository with 26 commits and 43 touches printed `over 43
commit(s)` — a number high enough to clear the twenty-commit floor that five
commits should not, printed under the word *commits*. `git log --numstat` lists a
line per file per commit, so the commits were countable from the hashes the format
string was already handing the parser and discarding. Counted, compared against
`git rev-list --count` in
`the_window_counts_commits_not_the_lines_they_occupy`.

Nothing else about the measurement was wrong, which is the uncomfortable part:
the per-file counts, the scores, the dates and the dependents were all right,
and the one wrong number was the one a reader would quote back at you.

### Measured

- 24 rules, 16 of which gate. `advisory 8`, up from 6.
- 139 tests across 9 binaries, 0 ignored. `cargo clippy --all-targets` clean.
- On `../arioch`: `churn-concentration` names 4 files, `app.rs` holding 32% of the
  crate's churn with 1 of its 8 committed files leaning on it, over 40 commits
  since 2026-08-26. `untested-port` is `Unevaluable` — "no port trait exists" —
  which is the correct answer for a codebase that has no ports, and is the reason
  the rule says what it says rather than what its name suggests.
- On this repository: `untested-port` is `Unevaluable` (no ports),
  `churn-concentration` reports nothing (no file is in the top decile on both
  axes — the churn is spread across 20 files and every one of them is depended on
  by fewer than a tenth of the maximum).

### What this spec did not do

No coverage percentage, because that needs a compiler. No attribution beyond a
direct call or an impl, so a port exercised only through its caller still reads as
unexercised — stated in every reason string rather than smoothed over. No window
other than all of history; `--since` is a flag nobody asked for yet. And the
dependent count that makes `churn-concentration` a hotspot is a count of *files
that import a file*, which is a proxy for coupling and says nothing about whether
the coupling is a good idea.
