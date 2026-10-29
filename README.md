# metatron-rust

Point it at a Rust crate and get a measured model of its architecture, then
check that model against `patterns-rust`, the layout convention these specs are
written against.

A Rust rewrite of [`metatron-nestjs`](../metatron-nestjs). Same idea — nothing
in the output is drawn or written by hand — but the unit of analysis is the
**symbol**, not the file, because a Rust file holds many types and the
architecture lives in traits and `impl` blocks rather than in filenames.

Read [`specs/README.md`](specs/README.md) first. It sequences the work and
explains why the target codebases have no architecture yet.

## Status

| spec | status |
|---|---|
| 01 symbol model and the `syn` scanner | **implemented** |
| 02 cohesion analysis (LCOM4) | **implemented** |
| 03 classifier and `metatron.toml` | **implemented** |
| 04 conformance rules and enforcement tiers | **implemented** |
| 05 scorecard, baseline, and `cargo test` | **implemented** |
| 06 retargeting the views | **implemented** |
| 07 trustworthiness and self-audit | **implemented** |
| 08 test presence and churn bounds | **implemented** |
| 09 logical coupling (co-change analysis) | **drafted** |
| 10 blast radius (`metatron diff`) | **drafted** |
| 11 the workbench (`metatron serve`) | **implemented** |

## Try it

```bash
cargo build --release
./target/release/metatron scan ~/path/to/a/crate
```

Writes `.metatron/model.json` and prints a summary. `--stdout` prints the model
instead of writing it.

```
metatron scan · arioch

  8 files · 8 modules · 5471 loc
  166 symbols (18 types, 146 fns) · 792 edges
  2 impl bindings
  coverage 67/83 symbols (80.7%) · 0 ports
  wiring 25/25 sockets placed · 0 port · 6 external · 0 unresolved
  studs 35 · 29 by a brick · 0 through a port · 0 entry · 1 foreign trait · 5 unseen

  16 symbol(s) matched no pattern, in 1 file(s):
    main.rs                        16x   e.g. main.rs:16 Cli
    Add a pattern in metatron.toml, or `metatron classify --verbose` for each one.

  externs
    ratatui                      482x
    std::fs                      28x
    std::path                    26x
```

## The workbench

```bash
./target/release/metatron serve ~/path/to/a/crate     # http://127.0.0.1:4477/
./target/release/metatron serve --port 5000 --no-watch
```

The views are pictures. The workbench is somewhere to pick the code up and
turn it over: the interactive brick graph from metatron-nestjs, read off Rust.
Pick a type, a trait, or a module's free functions, and it is drawn as a brick
in a stack, with what it sits on below it and what sits on it above. Rows are
the layers from `metatron.toml`.

| | metatron-nestjs | here |
|---|---|---|
| brick | a class, a file of functions, a port | a struct or enum with its `impl` blocks, a trait (hollow), a module's free fns (`module::*`) |
| socket | a constructor parameter | a field, or a parameter of one of its fns: `&impl ActivityStore` is a socket into the port |
| stud | a public method | a `pub` fn or method, a trait's declared fn, or an impl's fn of a trait |
| wire | an injection | a field holding, or a parameter taking, another brick |
| dotted line | a module binding a token | `impl Trait for Type`: every brick that fills the port |

Hover a stud to light every call that grips it, a socket to see which studs
it grips, a line or a legend item for what it means. Click anything to read
its source, where each call site links to the brick it lands in. It re-scans
when a `.rs` file, `Cargo.toml` or `metatron.toml` changes, and the open page
redraws with your place kept.

A call is followed through what is **declared**, never inferred: the type of
`self.<field>` or of a parameter, the type a `let` writes (`let x: Foo`,
`let x = Foo { .. }`), or the declared return type of the fn a local came from
(`let s = Scorecard::build(..)?`). A local whose `let` declares nothing
readable is counted, not followed. A port with one implementation outside tests is
bound to it; one with several is not, because which one runs is the
composition root's choice. `scan` prints the workbench's own coverage, so an
unplaced socket is a number before it is a gap on the bench.

It binds `127.0.0.1` and answers only requests addressed to it, because it
serves source code. There is no change overlay yet: that needs `metatron diff`
(spec 10), so the page does not offer one.

## What the model carries

Symbols (modules, types, fns, methods, statics) with visibility, `cfg`,
derives, fields, signatures, and — for methods — the `self.<field>` accesses and
`self.<method>()` calls that LCOM4 needs.

Six edge kinds. Five of them exist so the port seam is visible:

| kind | what it captures |
|---|---|
| `Use` | `use crate::x::Y` — the classic import edge |
| `Impl` | `impl Trait for Type` — dependency inversion, pointing backwards against layer order |
| `Field` | struct composition |
| `Sig` | parameter and return types |
| `Bound` | `&impl Store` / `&dyn Store` / `T: Store` — calling *through* the port |
| `Call` | best-effort call graph |

`Impl` and `Bound` are the two with no NestJS analog, and between them they hold
the entire hexagonal architecture. Neither produces an import.

## What it refuses to do

`syn` parses; it does not resolve names. Resolution here is a hand-built symbol
table plus each file's `use` map, and **anything it cannot place becomes a
diagnostic rather than a guess**. Macro-generated items are invisible and say
so. `#[cfg]` is recorded, not evaluated. Method dispatch through a trait object
is not attempted.

The failure mode of an architecture tool is not crashing — it is drawing a
confident picture of something it did not understand.

## Tests

```bash
cargo test
```

Among the fixture crates: `gnarly` (generics, lifetimes, turbofish, raw strings,
`macro_rules!`), `ports` (a minimal `patterns-rust`-shaped crate with a real
port seam), and `wiring` (one of each socket and stud the workbench tells apart). The arioch and enoch assertions skip if those checkouts are absent.

MIT.
# metatron-rust
