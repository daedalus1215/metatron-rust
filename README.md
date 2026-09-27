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
  coverage 1/83 symbols (1.2%) · 0 ports

  82 symbol(s) matched no pattern, in 8 file(s):
    main.rs                        16x   e.g. main.rs:16 Cli
    ui.rs                          16x   e.g. ui.rs:12 render
    app.rs                         14x   e.g. app.rs:6 TICK_RATE
    syntax.rs                      14x   e.g. syntax.rs:4 FileType
    knowledge.rs                    7x   e.g. knowledge.rs:3 Danger
    registry.rs                     7x   e.g. registry.rs:7 Entry
    config.rs                       6x   e.g. config.rs:5 CONFIG_OVERRIDE
    annotations.rs                  2x   e.g. annotations.rs:5 Annotation
    Add a pattern in metatron.toml, or `metatron classify --verbose` for each one.

  externs
    ratatui                      482x
    std::fs                      28x
    std::path                    26x
```

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

Two fixture crates: `gnarly` (generics, lifetimes, turbofish, raw strings,
`macro_rules!`) and `ports` (a minimal `patterns-rust`-shaped crate with a real
port seam). The arioch and enoch assertions skip if those checkouts are absent.

MIT.
# metatron-rust
