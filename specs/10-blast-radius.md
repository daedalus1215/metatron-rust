---
title: Blast Radius (`metatron diff`)
status: draft
project: metatron-rust
location: specs/10-blast-radius.md
created: 2026-10-18
tags: [ci, pr-review, impact-analysis, reachability, ratchet]
---

# Blast Radius (`metatron diff`)

## Context

`metatron check` already answers "is the tree worse than the baseline?" —
the fingerprint diff, the ratchet, and the exit code are spec 05. What no
command here can answer is the one a reviewer asks first:

> This change touches 3 files. This crate's CLI and TUI surface runs through
> them. It opens 1 new violation and closes 2. Two touched files are top-decile
> hotspots and neither has a test.

That is impact analysis. It is also the story a crate's history tells about
itself: reachability, hotspots, test presence, and the committed baseline all
exist in the model today, and none of them are connected to a change set.

The original idea — "point metatron at a PR and render just the touched
files" — is a filtered view, not an analysis: scoping answers the same
questions in a smaller frame and inherits all the lens problems spec 06
already owns. The version worth building needs the baseline from spec 05 to
say "adds" rather than merely "has", and the model from spec 01 to say
"depends on" rather than merely "sits nearby".

The Rust twin is metatron-nestjs's `docs/specs/04-blast-radius.md` **at its
Amendment (2026-09-27) state, not its first state**. That implementation
learned, the hard way, that the head of a range must be read from git objects
rather than the working tree; this spec starts there. It also restores the
honesty rules spec 07 fixed for this codebase: a scan difference is reported
as one.

## Goal

Given a change set, report what depends on it, which entry points execute it,
and how it moves the architecture relative to the committed baseline —
without touching the working tree and without posting anything anywhere.

- Deletions and renames are measured, not silently reported as zero impact.
  This is the hard part of the spec and is built first (Design, 2).
- A new violation in **untouched** code is labelled a scan difference, not
  blamed on the diff.
- Every number carries what it was computed from: the range, the commit range
  actually resolved, and which side of it (base or head) produced each
  reading.
- The command runs offline, never checks out, never stashes, and writes
  `.metatron/` state only when asked.

## Design

### 1. Input: git ranges only

```bash
metatron diff main...HEAD    # explicit range
metatron diff                # merge-base with the default branch...HEAD
metatron diff --staged       # what is about to be committed
```

The change set is `git diff --name-only <range>`, filtered to files inside the
scanned root with a repo-root pathspec, reported relative to the scanned root
so they join the model's keys directly. A naive implementation here silently
drops every file whenever the crate sits below the repository root — recorded
as the first deviation from the twin spec, built correctly from the start.

No forge integration. A range works on any local branch, offline, against any
remote, and before a PR exists — which is when the answer is most useful.
Resolving a PR number to a range is a one-line alias for whoever wants it.

### 2. The scanner reads objects, not the working tree (built first)

`Scanner::walk_module` reads files through `std::fs::read_to_string`
(`src/scan/mod.rs:506`). That single call site gains a seam: the scanner
optionally carries an in-memory file map (`rel path -> content`), and when it
does, the map is the crate. No checkout, no stash, safe to run mid-edit.

`diff` scans **both sides of the range this way**:

- **base:** `git merge-base`, then a batched `git cat-file --batch` into the
  file map, which is how a deleted file's importers — numbers that can only
  exist on the base side, and the most dangerous possible wrong answer if
  silently zero — are still visible.
- **head:** the range's head commit (`rev-parse <head>^{commit}`), again
  through objects; for `--staged`, the index (`ls-files -s`, stage 0).

This is the twin spec's 2026-09-27 amendment, adopted as the initial design
rather than learned again: as first written there, the head was the working
tree, and a range whose head was not checked out measured nothing at all.

Churn (and, when landed, coupling), is bounded at the head commit or at HEAD
for the index — the twin's `opts.churnAt`, likewise carried in.

Runtime roughly doubles vs one scan and that is accepted: the report is
either trustworthy under deletions and renames or it is noise.

### 3. Analysis

**Reverse reachability.** Invert `Model::file_links` and walk outward from
the change set, with depth reported because direct and transitive are
different risks:

```
directly imported by      6 files
transitively reachable   41 files  (of 83 scanned)
```

**Surfaces affected.** metatron has no HTTP routes, but `views/traffic.rs`
already names the analog precisely: an inbound signal is a CLI subcommand
(`cmd_*`) or a TUI key handler (`handle_*`), traced through `Call` edges to
the port it satisfies (`Entry.flat`). A surface is affected when any changed
file appears in its trace or is its own file, and the report names `via`
which changed files the path runs through. Traces that could not be resolved
report as diagnostics and are drawn as nothing — the existing rule, matched
by the new report.

**Violation delta with attribution.** Scan both sides, fingerprint as spec 05
does, and attribute each new violation to whether it involves a changed file
(instance `file:line` ∈ change set, directly or through its type's file). A
new violation in untouched code is a **scan difference** and is labelled as
such — a label, not a guess about why.

**Rename folding.** A pure rename legitimately changes every fingerprint of
the moved symbols while changing nothing about the architecture. When a base
symbol's fingerprint reappears as new under a different symbol id with the
same rule endpoints modulo the moved module prefix, the pair is folded and
reported in the renames section, not as +1/−2. Folding applies only with a
baseline present; without one there is nothing to fold against and the
report says so — the twin's recorded degradation.

**Risk crossings.** Per changed file: its hotspot rank (spec 08's churn
distribution), its co-change partners from spec 09 absent from the diff, and
whether any scanned test target reaches it (spec 08's test presence). The
co-change line is the highest-value sentence in the report — *"you changed
`a.rs`; it has changed with `b.rs` in 14 of the last 19 commits, and `b.rs`
is not in this diff"* — which is why spec 09 lands first.

### 4. Output

Default: a terminal report. `--format=markdown` for a PR description;
`--json` for tooling.

```markdown
### metatron · 3 files changed

**Blast radius** — 6 direct dependents, 41 transitively reachable (12.5% of the tree)

**Surfaces affected** — 3 · `cmd_ls` via scan/registry.rs · `cmd_open` via … .

**Architecture** — +1 / −2 vs baseline
- new  `domain-no-io` scan/commands.rs → uses rusqlite directly
- mixed-layer   scan/commands.rs spans domain + infrastructure

**Worth a look**
- `rules.rs` is a hotspot on both sides (bottom decile by score; 10 dependents)
- `rules.rs` co-changes with `model.rs` 60% of the time — not in this diff
- 2 of 3 changed files are not reached by any scanned test
```

Nothing is posted anywhere: metatron writes stdout, a CI step decides what to
do. No credentials, no network.

### 5. Exit code

`diff` never gates by default (exit 0 on a clean scan; non-zero on a scan
failure). Wiring it to the ratchet — fail a range whose head introduces a
new fingerprint — is a deliberate edit owned by the gating table (spec 04),
not by this command; `check` remains the enforcement surface.

## Model changes

No change to the scanned model; `diff` composes existing structures. New
module `src/diff.rs` with:

```rust
pub struct DiffReport {
    /// The literal range argument, and what it resolved to.
    pub requested: String,
    pub base_commit: String,
    pub head_commit: String,   // or "(index)" for --staged
    pub change_set: Vec<String>,   // root-relative
    pub direct: Vec<String>,
    pub transitive: Vec<String>,
    pub surfaces: Vec<Affected>,   // Entry + via list
    pub delta: Delta,              // new/known/fixed, attribution per instance
    pub renames: Vec<Rename>,      // folded fingerprint pairs
    pub crossings: Vec<Crossing>,  // hotspot / co-change / test presence
    pub generated_at: String,
}
```

and the three renderers beside it, in the same file, the way `baseline` keeps
its formatting near its data.

## Honesty constraints

- Every reading names its side: base-side facts say `base`, head-side facts
  say `head`, and deltas name the baseline they were compared against.
- The report states the trust boundary of the trace engine: `Call` edges are
  best-effort (spec 01), so surface lists are "known routes through", not
  "all routes through" — and a trace that cannot resolve is a diagnostic,
  never a guess.
- A change set filtered against the wrong root is the failure mode
  everything above is most deadly to: the report states the pathspec it used
  and the count it produced, so "0 files changed" is checkable.
- Absence of a baseline is not a silent zero delta; it is a labelled
  degradation with the reason (the five absences churn distinguishing already
  learned).
- Renames folded, added/fixed counts exclude the folded pair — the twin's
  acceptance criterion about a pure rename reporting **near-zero** impact is
  the direct test.

## Dependencies

- **01** — the model; misresolved use edges fabricate reach.
- **05** — the violation delta is meaningless without a baseline; degrades
  gracefully without one.
- **08** — test presence and hotspot rank, for the crossings; absent history
  degrades those lines, not the report.
- **09** — co-change partners for the crossings. Absent coupling degrades to
  a silent gap, not an error.

## Out of scope

- A filtered lens (`?focus=<files>` on the city view) — later, as a view
  concern (the twin's spec 08), not here.
- Continuous monitoring or gate wiring — the ratchet-as-test (spec 05) is the
  enforcement mechanism; `diff` informs.
- Blame or authorship; forge integration of any kind.

## Acceptance

All against fixture crates with scripted, synthetic git histories, mirroring
the twin spec's fixture shape — and at least one against arioch's real
history:

- `metatron diff HEAD~1...HEAD` on a known commit reports a direct/transitive
  set that matches a hand-written reverse BFS over `file_links`.
- A commit deleting a depended-upon file reports **non-zero** blast radius,
  computed from the base-side file map.
- A pure-rename commit reports its renames under `renames` and **no**
  added/fixed pair for the moved fingerprints — `added == 0 && fixed == 0`
  asserted directly.
- A range whose head is not checked out, and an uncommitted edit to a file in
  the range, each report identically (the amendment as acceptance tests).
- A change set from a crate sitting below the repository root still matches
  the diff pathspec (the root-prefix deviation as an acceptance test).
- No baseline present: the report says "no delta" with the reason, and lists
  current violations touching changed files.
- Runs with no network access; never touches the working tree (a test asserts
  the tree is byte-identical after a full run, watched directory as churn's
  sandbox does).
- On metatron-rust itself: `metatron diff HEAD~1...HEAD` produces a report
  and the tool's own `check` remains green.
