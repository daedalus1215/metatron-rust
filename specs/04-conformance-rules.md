---
title: Conformance Rules and Enforcement Tiers
status: implemented
project: metatron-rust
location: specs/04-conformance-rules.md
created: 2026-08-29
tags: [rules, fitness-functions, enforcement, patterns-rust]
---

# Conformance Rules and Enforcement Tiers

## Context

This is where `patterns-rust` becomes executable.

The glossary does something metatron does not, and it is the best idea in it:
every pattern page ends with an **Enforcement** section naming the *mechanism*
that makes the rule hold — compiler, trait, visibility, lint, or convention. Its
README is explicit about why: "The Enforcement section is what separates a
strict pattern flow from a naming guide."

That axis should be first-class in the tool, because it answers a question no
architecture tool currently answers: **which of my rules is anything actually
enforcing?**

### The catch

`dependency-hierarchy.md` lists three rules as **compiler**-enforced:

| Direction | Allowed? | Enforced by |
|---|---|---|
| domain → infra | NO | **compiler** |
| domain → application | NO | **compiler** |
| infra → application | NO | **compiler** |

That is true only under `crate-graph.md` **Option B** — a workspace split where
`domain` is a crate that does not depend on `rusqlite`. arioch and enoch are
single-crate, which is **Option A**, where the same page prescribes "visibility
+ a source-scan test."

So today, in both target codebases, **zero rules are compiler-enforced. Every
rule in the glossary is convention.** That is the gap this tool exists to fill,
and the tool says so out loud on every run — arioch, 2026-09-27:

```
enforcement            compiler 0 · metatron 16 · clippy 0 · advisory 6
                       ^ no rule is enforced by anything but this tool
```

That line inverts as the project matures. Do the workspace split and the
compiler absorbs three rules, at which point metatron reports them as
`compiler` and stops checking them — a rule the build already guarantees does
not need a second opinion, and reporting it as a metatron pass would overstate
the tool's contribution.

## Goal

Evaluate the glossary's rules against the model, tagged by enforcement tier, and
separate what can be *decided* from what can only be *smelled*.

## Design

### Decidable versus heuristic

Every rule carries a `kind`:

- **`decidable`** — the model contains enough to be sure. May gate a build.
- **`heuristic`** — a smell with real false-positive rate. Reported, never gated,
  regardless of configuration.

This split is non-negotiable and is the reason the tool survives contact with a
real repo. A ratchet that fails CI on a guess gets disabled within a week, and
takes the decidable rules down with it.

The uncomfortable consequence, stated plainly: **the violation the glossary
cares most about is heuristic.** "The store is making a business decision" —
`Db::start` returning `StartOutcome::AlreadyRunning` — is cited as an
anti-pattern on `store.md`, `use-case.md`, and `service.md`. It is semantic.
The best available proxy is "an `infra/` method returns a domain enum, or
branches on domain state," which is a genuine signal and a fallible one. It gets
reported prominently and gates nothing.

### The rule table

| id | rule | source | tier | kind | gates |
|---|---|---|---|---|---|
| `domain-no-io` | no symbol in `domain/` has an `Extern` edge to an `io` or `render` crate | dependency-hierarchy, port | compiler (B) / lint (A) | decidable | ✅ |
| `domain-no-application` | no `domain/` symbol references an `application/` symbol | dependency-hierarchy | compiler (B) / lint (A) | decidable | ✅ |
| `infra-no-application` | no `infra/` symbol references an `application/` symbol | dependency-hierarchy | compiler (B) / lint (A) | decidable | ✅ |
| `concrete-outside-root` | no concrete store/adapter struct is named outside `infra/` and the composition root | port.md — *"the defining violation"* | trait + lint | decidable | ✅ |
| `call-through-port` | a `use-case`/`service`/`command-handler` parameter typed as a concrete infra struct rather than `&impl P` / `&dyn P` | port.md rule 2 | trait | decidable | ✅ |
| `flow-skip` | derived from `flow`; an edge jumping ≥1 station | dependency-hierarchy | lint | decidable | ✅ |
| `no-same-level` | use-case→use-case, service→service, store→store, handler→handler, via `Call` edges | dependency-hierarchy rule 1 | convention | decidable | ✅ |
| `port-signature-purity` | no port trait method signature names a non-domain type (`rusqlite::Connection`, `ratatui::Frame`) | port.md | compiler-adjacent | decidable | ✅ |
| `no-global-mut` | no `static mut`, and no `static` holding `Mutex`/`RwLock`/`OnceCell` of mutable state | design-philosophy principle 3 | convention | decidable | ✅ |
| `dto-in-domain` | no symbol named `*Dto`/`*DTO` under `domain/` | naming.md | lint | decidable | ✅ |
| `use-case-verb` | every `use-case` fn name begins with an allowlisted verb | naming.md | lint | decidable | ✅ |
| `time-injected` | no direct `Local::now()` / `SystemTime::now()` / `Instant::now()` outside `infra/` | use-case.md, testing.md | convention | decidable | ✅ |
| `converter-is-pure` | no symbol classified `converter` takes a port, returns `Result`, or has an `io` extern edge | converter.md | convention | decidable | ✅ |
| `port-has-fake` | every port trait has ≥2 impls, one of them `is_test` or in `infra/mem.rs` | testing.md — *"one fake per port"* | convention | decidable | ⚠️ warn |
| `mixed-layer-module` | no module contains symbols from >1 layer | design-philosophy, spec 02 | convention | decidable | ✅ |
| `store-decides` | an `infra/` method returns a domain enum or branches on domain state | store.md, use-case.md | convention | **heuristic** | ❌ |
| `handler-decides` | a `command-handler` contains a `match` on a domain enum | command-handler.md | convention | **heuristic** | ❌ |
| `service-wraps-one` | a `service` method whose body calls exactly one use-case | service.md — *"ceremony without a workflow"* | convention | **heuristic** | ❌ |
| `fat-trait` | a port trait exceeding N methods (default 12) or spanning >1 extern concern | port.md — interface segregation | convention | **heuristic** | ❌ |
| `renders-off-store` | a render fn reaching a store/entity field path (`app.registry.entries`) | view-model.md | convention | **heuristic** | ❌ |

Twenty rules: **fifteen decidable, five heuristic.** Fourteen gate, one warns,
five never gate.

`panic-in-domain` is listed under `tier: clippy` because `clippy` ships
`unwrap_used` / `expect_used` and could do the job. It is decided here anyway:
the scanner records `unwrap`/`expect` call sites on each symbol, and reading
them back is one comparison. Marking it `Delegated` on the grounds that a
`clippy.toml` could deny the lint per module was the mistake spec 07 exists to
catch — this repository configures no clippy lints, so nothing was enforcing
it, and a rule delegated to nothing has no premise and is dark in every crate.

### The `Impl` edge

Spec 03 exempts `Impl` edges from `flow-skip`. They get their own treatment
here, because they are not a violation to tolerate — they are the architecture
succeeding, and they should be counted as such:

- `impl <domain trait> for <infra struct>` → **upheld**, the dependency-inversion
  arrow. Report under a `good` finding, and count them: "7 ports, 7 real impls,
  6 fakes."
- `impl <infra trait> for <domain struct>` → **violation**. The domain is
  implementing infrastructure's interface, which inverts the inversion.
- A port trait with exactly one impl and no fake → `port-has-fake`.
- A port trait with zero impls → dead port, reported as a note.

### Reporting an unevaluable rule

Carried from spec 03 and load-bearing for the current state of both codebases: a
rule whose premise is absent reports **`unevaluable`**, never `pass`. With zero
ports in arioch, `call-through-port`, `port-signature-purity`, `port-has-fake`,
`fat-trait`, and `concrete-outside-root` all have nothing to evaluate. Reporting
five green checks there would be the single most misleading thing this tool
could do.

```
metatron check · arioch
  rules           22     upheld 0 · violated 3 · unevaluable 19 · pass 0
  enforcement            compiler 0 · metatron 16 · clippy 0 · advisory 6
                         ^ no rule is enforced by anything but this tool

  violations       3     new 3 · known 0 · fixed 0

  cohesion         1     type(s) over threshold
                         App  app.rs:50  3 components, 46 methods  [Tangled]

  NEW
    time-injected          app.rs:1925  app::iso_now calls std::time::SystemTime::now directly
                           51533f5c65e0
    no-global-mut          config.rs:5     CONFIG_OVERRIDE: static holding Mutex — `std::sync::LazyLock<parking_lot::Mutex<Option<PathBuf>>>`
                           550777db98cf
    time-injected          app.rs:244   app::App::log_action calls std::time::SystemTime::now directly
                           d9fc49a718ca
```

### Findings shape

Retains metatron's structure so spec 05's baseline and spec 06's templates need
no adaptation: each rule produces a finding with `id`, `tone`
(`good`/`note`/`warn`), `title`, `detail`, `items[]` for display, and
`instances[]` of `{from, to, file, line}` for fingerprinting. New fields:
`tier`, `kind`, and `gate`.

## Out of scope

- Auto-fixing anything.
- Cross-crate rules for a workspace split. When Option B happens the three
  compiler rules move to `tier: compiler` and stop being checked; the
  cross-crate *resolution* work that enables the rest is a change to spec 01.
- Coverage gating on `domain/` (testing.md suggests it). That is `cargo tarpaulin`
  or `cargo llvm-cov`, and wrapping another tool's number is not this tool's job.

## Acceptance

Against arioch as it stands, the following must be found — each already
diagnosed by hand in the glossary, which is what makes them a test:

- **`no-global-mut`** fires on `CONFIG_OVERRIDE` (`design-philosophy.md`
  principle 3 names it).
- **`domain-no-io`** reports `std::fs::read_to_string` and the `$EDITOR` process
  spawn inside `app.rs` once a `domain/` exists — and reports `unevaluable`
  before that, rather than passing.
- **`renders-off-store`** flags `ui.rs` reading `app.registry.entries`
  (`view-model.md` cites exactly this).
- **`use-case-verb`** reports `unevaluable`: arioch has no
  `domain/use_cases/`. *(The criterion originally expected it to flag
  `Registry::scan_with_config`. That is `naming.md`'s complaint about
  positional parameter lists, which is a different rule and not in this
  table.)*
- **`concrete-outside-root`** reports `unevaluable` — arioch has no ports, so
  there is no concrete-versus-port distinction to violate yet. It must **not**
  report a pass.

And on the conforming fixture crate from spec 03:

- All 15 decidable rules evaluate; none are `unevaluable`.
- Introducing `use rusqlite::Connection` into `domain/use_cases/activity.rs`
  fires `domain-no-io` and names the file and line.
- Changing `fn start_activity(store: &impl ActivityStore)` to
  `fn start_activity(store: &SqliteActivityStore)` fires both
  `call-through-port` and `concrete-outside-root`.
- Adding a second use-case call inside a use-case fires `no-same-level`.
- Deleting the `MemStore` fake fires `port-has-fake` as a warning, not a failure.
- A store method returning `StartOutcome` produces a `store-decides` finding
  that is **reported and does not affect the exit code**, under any flag.


---

## Implementation notes (2026-08-31)

`src/rules.rs`, `metatron check [path] [--all] [--json]`, exit 1 on a
gating violation. 21 tests in `tests/rules.rs` and a new
`tests/fixtures/leaky` crate — the conforming layout with every violation
in the table deliberately introduced.

**22 rules, not 20.** `dependency-inversion` was implicit in the spec's
"the `Impl` edge" section and is now a rule that can be `Upheld`;
`panic-in-domain` was carried as `Delegated` so the coverage story stays
complete, and became a decided rule in spec 07 once the model recorded the
calls it was delegating about.

### Measured

```
metatron check · arioch
  coverage         1/83 symbols (1.2%)  !

  rules           22     upheld 0 · violated 3 · unevaluable 19 · pass 0
  enforcement            compiler 0 · metatron 16 · clippy 0 · advisory 6
                         ^ no rule is enforced by anything but this tool

  violations       3     new 3 · known 0 · fixed 0

  NEW
    time-injected          app.rs:1925  app::iso_now calls std::time::SystemTime::now directly
    no-global-mut          config.rs:5  CONFIG_OVERRIDE: static holding Mutex
    time-injected          app.rs:244   app::App::log_action calls SystemTime::now directly
```

Four heuristic findings are reported under `ADVISORY` and excluded from the
ratchet, and the six rules that never gate are listed under `outside the
ratchet`; both blocks are cut here for width.

**Nineteen of twenty-two rules are unevaluable against arioch.** That is
the correct output and the reason the `Unevaluable` status exists: arioch has
one classified symbol, so a rule about the domain has nothing to look at. The
conforming fixture inverts it: zero unevaluable, zero gating violations,
five upheld inversion arrows. The leaky fixture fires every decidable rule
that has a premise there, plus every heuristic.

The block above is a real run of the current binary (2026-09-27), not an
earlier draft of one. The output format changed twice since this section was
written — the enforcement line and the `outside the ratchet` list did not exist
— and the numbers moved with it: `panic-in-domain` stopped being delegated, so
the advisory count fell from 7 to 6, and a rule became decidable, so the
unevaluable count rose.

### The spec's `flow-skip` derivation contradicts its own source

Spec 04 derives the rule as "an edge jumping >= 1 station in `flow`". Run
literally, **it fires on the conforming fixture**:

```
command-handler -> port  skips 2 station(s)
command-handler -> use-case  skips 1 station
```

Both are explicitly permitted by the dependency matrix in
`dependency-hierarchy.md`, which lists a command-handler's allowed
dependencies as "services, use-cases (simple), ports (to fetch)". Every
downward pair above the seam is legal.

The flow is not a pipeline. It is a partial order with **one seam in it**,
and this spec already says so in the sentence that follows the derivation:
*"`port` is a permitted terminus."* The rule's actual content is therefore:
no edge reaches **past** the port to what implements it. That fires four
times on the leaky fixture (use-case, service and handler all naming
`SqliteActivityStore`) and never on the conforming one.

### Trait methods were being counted as ports

Spec 03 has methods inherit their type's classification, so filtering on
`pattern == "port"` collected every *method* of every port trait. The
conforming fixture reported seven `port-has-fake` violations, one per
trait method, each phrased as a dead port. Ports are now filtered on
`kind == Trait` as well.

### Two heuristics needed data the model did not carry

- `handler-decides` and `store-decides` need to know what a body branches
  on. `BodyScan` now records the enum paths in `match` arm patterns.
- `renders-off-store` needs the reach through the object graph.
  `BodyScan` now records dotted chains two fields deep or more.

Both immediately exposed the same two gaps as spec 02:

1. **Free functions never recorded their body scan into the `Symbol` at
   all.** Spec 01 populated `self_fields` and friends only for methods,
   which was harmless while the only consumer was LCOM4 — a free function
   has no `self`. `cmd_start` is a free function, and `handler-decides`
   silently found nothing.
2. **The chains were inside `format!` again.** `render_row`'s
   `row.store.conn.path` is in a macro, invisible to the AST visitor. The
   token walker from spec 02 now builds whole chains, not just single
   accesses.

A chain ending in a call is not a reach: `entry.tags.join(", ")` gets at
one field and then does something with it. Before that fix arioch reported
nine `renders-off-store` findings, five of them noise. It reports four
now, and **all four reach `app.registry`** — which is the exact expression
`view-model.md` cites.

### `use-case-verb` had to be evaluated against the directory, not the classifier

The spec 03 classifier only calls something a use-case *if the name
matches the verb allowlist*. Checking the classifier's output for verb
conformance is circular and can never fire. The rule runs over every `fn`
in `domain/use_cases/` and reports the ones the classifier declined.

### The enforcement line

```
enforcement            compiler 0 · metatron 16 · clippy 0 · advisory 6
                       ^ no rule is enforced by anything but this tool
```

Zero, because both target crates are `crate-graph.md` Option A. The four
numbers are a partition of the 22 rules, each counted once by whoever enforces
it: a `Delegated` rule is counted under the tool that took it, not under this
one.

Setting `crate_graph = "B"` in `metatron.toml` moves the three layer rules to
`tier: compiler` and `status: delegated`, and the line becomes
`compiler 3 · metatron 13 · clippy 0 · advisory 6` — a rule the build already
guarantees does not need a second opinion, and reporting it as a metatron pass
would overstate the tool's contribution.
`the_workspace_split_hands_three_rules_to_the_compiler` in `tests/rules.rs`
asserts that transition rather than this paragraph, so the numbers here cannot
drift away from the code without the test noticing the code moved.

It is not auto-detected. Being a workspace is not the same as having
`domain` as a crate that cannot see `rusqlite`, and guessing here would
let the tool claim an enforcement it has not verified.

### Profile additions

`converter` and `renderer` patterns, both from pages the spec 03 profile
had skipped (`converter.md`, and `view-model.md`'s "consumed by render
fns"). Without them `converter-is-pure` was permanently unevaluable and
render functions were unclassified.
