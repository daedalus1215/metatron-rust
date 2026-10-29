---
title: Logical Coupling (Co-Change Analysis)
status: draft
project: metatron-rust
location: specs/09-logical-coupling.md
created: 2026-10-18
tags: [analysis, git-history, coupling, heuristic, dsm]
---

# Logical Coupling (Co-Change Analysis)

## Context

Every graph this tool draws — layers, city, atlas — is built from
import edges (`Model::file_links`, `src/model.rs:257`, "the one definition of
imports in this codebase"). That is one definition of coupling, and it is
structurally blind to another: **two files with no import edge between them
that keep changing together.**

That pattern is usually a missing abstraction, a duplicated invariant, or
shotgun surgery. It cannot be found by reading the code — only by reading its
history — and no lens here exposes it.

The data is already being parsed and thrown away. `Churn::log`
(`src/churn.rs:239`) walks `git log --no-merges --numstat` and, per commit,
reduces everything to per-file totals: at `src/churn.rs:292-301` a file's
counts are accumulated and the commit grouping that identifies co-change is
discarded — only `commits: BTreeSet<String>` of hashes survives
(`src/churn.rs:268`), which is exactly the thing counts commits, not what
changes together. The set of files in each commit is computed and dropped.

Both target crates have real, if young, histories: arioch has 22+ commits, a
window measured honestly by `Window.since` (`src/churn.rs:214-218`). The
design must read a thin sample without over-claiming on it — the same discipline
spec 08 applied to churn, which is why this spec extends that one's honesty
contract rather than inventing a new one.

This spec is the Rust twin of metatron-nestjs's `docs/specs/03-logical-coupling.md`,
including the corrections its implementation notes recorded there (support
counted over trusted commits only; the floor actually used stored in the model).

## Goal

Report which files change together more often than coincidence explains, and
surface where that disagrees with the import graph — as a heuristic finding
that warns and never gates, and as a lens drawn from the model, not typed
prose.

- A pair is reported with the evidence that produced it: how many commits, over
  how many trusted commits, and the degree. No bare percentages without
  denominators.
- Coupling that agrees with an import edge is data, not a finding. The finding
  is coupling **without** an edge.
- Where the history is absent, young, or wide-commit-dominated, the rule and
  the lens say so: `unevaluable` and a stated reason, not an empty chart under
  a confident caption (spec 07's failure mode).
- Commits are a proxy for change, qualified in the lens itself: commit
  granularity is a team habit, not a property of the code.

## Design

### Extraction: keep the per-commit file set

`Churn::log` keeps walking the same command, but each `\u{1}`-separated commit
header collects its files into a per-commit vector, and the map of files
returns alongside it. The parse is unchanged; only the discard goes away.

Two guards, both carried from the twin spec:

**Commit size cap.** A 200-file rename or a formatting sweep couples
everything to everything. Commits touching more than `max_files` files are
ignored for pairing (default **25**), and the excluded count is reported: a
corpus where half the commits are excluded is telling you something.

**Support floor.** A pair that co-changed 2 of 2 times is 100% coupled and
means nothing. Both files must have changed at least `min_changes` times among
**trusted (non-ignored) commits** — support over the trusted commits only, so
a file dragged along by a sweep does not earn eligibility
(the twin spec's recorded deviation, adopted here from the start).

### Scoring: Jaccard over commit sets

Raw counts favour files that simply change a lot. The reported degree is the
Jaccard similarity of the two commit sets:

```
degree(a, b) = co(a, b) / (changes(a) + changes(b) - co(a, b))
```

Symmetric, bounded to 0..1, so a threshold means the same thing in a
two-year-old crate and a two-week-old one. The asymmetric reading is stored
alongside, because it is often the more useful sentence:

```
when_a_then_b = co(a, b) / changes(a)
```

A repository that always drags its config along is a different story from a
config dragged along by six unrelated commands.

### The crossing against the import graph

The signal is a pair at or above `min_degree` (default **0.3**) with **no
`file_links` edge in either direction** — and the pair is labelled with
whether it crosses a module boundary, because cross-module co-change is the
finding a reviewer can act on and same-module coupling is usually a posture
about internal layout.

This is a **heuristic**, not a decidable rule: false positives are real (a
shared test fixture, a coordinated rename that survived the cap). It is held
in the rule table as `Kind::Heuristic` and therefore never gates a build —
"a ratchet that fails on a guess is switched off within a week," as the
hotspots view already says.

| status | when |
|---|---|
| `Unevaluable` | history absent, fewer than `Churn::MIN_COMMITS` trusted commits, or no file reaches the support floor — with the reason string naming which |
| `Pass` | enough history, and every eligible pair agrees with an import edge |
| `Violated` | at least one eligible pair with no import edge in either direction |

### The lens: a dependency structure matrix

Co-change is **pairwise**; every existing lens here is node-centric (scatter,
tower, plane) or hierarchical (atlas). A pair cannot be drawn as a point,
which is why this earns a new template (`templates/coupling.html`, wired into
the `VIEWS` table the same way the others are) rather than a panel inside
hotspots.

Form: files on both axes ordered by module so blocks are visible, cells shaded
by degree. Off-diagonal blocks are the cross-module coupling worth finding.
Readability is the real risk — mitigations, in order of importance:

1. **Only eligible files appear.** After the support floor the axis is a
   fraction of the crate, not the crate.
2. **Module grouping default.** The module × module matrix is always readable;
   file level is the drill-down, not the landing state.
3. **Threshold slider** on degree, opening at the `min_degree` that was
   actually used (stored in the model, not assumed).
4. **Import overlay on by default** — cells that also share an import edge
   shaded differently, so the unexplained ones are the first thing seen.

If the file-level matrix proves unreadable on a real crate, module level stays
primary; that decision is deferred to the rendered screenshot, not made now.

The template is static HTML with a `__DATA__` token like every other view —
no canvas dependency for the render check (`tools/render-check.mjs`); the
matrix is a table, not a canvas.

## Model changes

```rust
pub struct CouplingPair {
    pub a: String,
    pub b: String,
    /// Shared trusted commits.
    pub co: usize,
    pub changes_a: usize,
    pub changes_b: usize,
    // Jaccard over commit sets; the often-more-useful asymmetric one.
    pub degree: f64,
    pub when_a_then_b: f64,
    pub when_b_then_a: f64,
    /// 1 when a `file_links` edge exists in either direction.
    pub imports: u8,
    /// True when the pair sits in different modules.
    pub cross_module: bool,
}

pub struct Coupling {
    pub pairs: Vec<CouplingPair>,
    pub window: Window,          // the same window churn measured
    pub trusted_commits: usize,
    pub excluded_commits: usize, // the size cap's cost, always shown
    pub eligible_files: usize,
    pub min_files: usize,        // the cap actually applied
    pub min_changes: usize,      // the floor actually applied
    pub min_degree: f64,         // the threshold the lens opens at
    pub generated_at: String,
    pub available: bool,
    pub reason: String,
}
```

Computed in the scorecard pass next to `Churn::measure` and handed to the
rule and the lens — one measurement shared, so the finding and the picture
cannot disagree. Empty with a populated `reason` when churn is unavailable,
the same way churn degrades (`Churn::unavailable`).

## Config

metatron.toml, kebab-case like every key here:

| key | default | meaning |
|---|---|---|
| `[coupling] max_files` | `25` | ignore commits touching more than this |
| `[coupling] min_changes` | `5` | support floor per file, trusted commits |
| `[coupling] min_degree` | `0.3` | degree floor for the model; the finding shortlists above the reporting bar the narrated slot states |

Absent keys run the profile defaults; a crate with no config gets the same
defaults it gets for everything else.

## Honesty constraints

- Commits are a **proxy** for change. One team's commit granularity is not
  another's: a squash-merge workflow and a commit-per-thought workflow produce
  different numbers from identical work. The lens says so in a `data-narr`
  slot, not in typed prose — the mechanism exists precisely so text does not
  survive a data change that made it wrong.
- Co-change is **correlation**. Two files touched in the same sweeping rename
  are not coupled in any meaningful sense; the size cap blunts this,
  imperfectly, and the excluded-commits count is how the reader sees how
  imperfectly.
- Absence of coupling is not evidence of independence. It may be young code —
  the reason an immature history is `unevaluable` rather than a clean pass.
- The rule states, in its own reason string, the weaker thing it actually
  established.

## Dependencies

- **08** — the window, `MIN_COMMITS`, and the degradation pattern it set.
- **01** — `file_links`, for the crossing and the module map.
- Nothing here needs classification: coupling works on an unclassified crate,
  which is where most of it will be examined first.

## Out of scope

- Author-based coupling (files touched by the same person) — a different and
  more socially fraught analysis.
- Symbol-level coupling: files are the unit because `--numstat` gives files,
  and a symbol-level history would mean line-mapping every commit.
- Coupling across the git boundary (other repos, workspaces of other crates) —
  the log is scoped to the crate's own root the same way churn is
  (`src/churn.rs:196-199`).
- Gating. This is a heuristic and stays advisory; if it is ever promoted, that
  is a deliberate edit to the gating table (spec 04) with its own evidence.

## Acceptance

- A fixture crate with a synthetic, scripted git history (like the way
  churn's bounds were tested) producing each guard exactly once: the sweep
  exclusion, the support floor, the degree floor, the import crossing, and
  `max_files` as a config override. Exact-value degree assertions double as
  the sweep check: if the ignored commit were counted, the headline pair
  reads a different number.
- On arioch's own history, `coupling.pairs` is produced with the excluded
  count a small minority of the window's commits, and the rule does not gate:
  the gating table (spec 04) is untouched and `metatron check` still exits 0
  on the same tree.
- Deleting `.git` degrades to the same message class churn already produces —
  `unevaluable` with its reason, not an empty chart.
- The lens renders from `views`, with narration filled from the model; the
  narration paragraph changes when the threshold does, which is the test the
  first narration implementation forced this project to have.
- metatron-rust's own tree produces the rule honestly: if it reports
  `unevaluable`, the reason names the youth of the history, not a missing
  measurement.
