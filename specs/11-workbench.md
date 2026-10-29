---
title: The Workbench (`metatron serve`)
status: implemented
project: metatron-rust
location: specs/11-workbench.md
created: 2026-10-28
tags: [lens, server, workbench, wiring, interactive]
implemented: 2026-10-28
---

# The Workbench (`metatron serve`)

## Context

metatron-nestjs's most used lens is not one of its seven pictures. It is the
workbench: a local page where you pick up a class, see what it is snapped
into, hover a part to see what grips it, and read the source one click away.
Its specs 09 (the wiring model) and 10 (the workbench) built it. Nothing in
this repository did.

Every lens here so far is a file with its data baked in, which suits a
picture you share and does not suit a workbench, for the two reasons
metatron-nestjs's spec 10 gives: reading code needs the source, and a
workbench should follow the code as it changes. So this lens is served.

What does not port is the wiring model. metatron-nestjs reads it off
constructor injection: a constructor names what it needs and a module
decides what fills it. Rust has no injector. A type is built from what its
**fields** hold, and a function is handed what its **parameters** take. That
second one is the port seam `patterns-rust` is built on: `fn
cmd_start(store: &impl ActivityStore)`.

This spec is the Rust twin of metatron-nestjs's specs 09 and 10 together.
Their spec 11 (the change overlay) is not ported. It needs `metatron diff`,
which is spec 10 here and still drafted.

## Goal

`metatron serve` opens a local page that draws any type, trait, or module's
free functions in place: what it sits on, what sits on it, which part of
which brick grips which, and its source. The page re-scans on save. Every
connection drawn comes from something written in the source, and anything
the scan could not place is counted, not drawn as if it were absent.

## Design

### The mapping

| metatron-nestjs | here |
|---|---|
| a class | a struct, enum or union, with every method of its `impl` blocks |
| a port (interface + token) | a trait: a hollow brick whose studs are its declared fns |
| a file of exported functions | a module's free functions, one brick per module, id `module::*` |
| a constructor parameter (socket) | a field, or a parameter of one of the brick's fns |
| a public method (stud) | a `pub` fn or method, a trait's declared fn, or the fn of a trait impl |
| `this.x.method()` | `self.<field>.m()`, `<param>.m()`, `<local>.m()`, `Foo::new()?.m()` |
| a module binding a token | `impl Trait for Type` |
| `framework` (Nest provides it) | `external`: a type from another crate |

Test code is not a brick. A fake under `#[cfg(test)]` that implements a port
is not a binding of it either: with `FixedClock` in a test module and
`SystemClock` in `clock.rs`, the `Clock` port is bound to `SystemClock`.

### Sockets

One socket per thing a brick plugs into, not per field. Two fields of the
same type are one socket listed as `from, to`, and a port taken by five fns
of a module is one socket.

| status | meaning |
|---|---|
| `resolved` | a struct, enum or union in the tree |
| `port` | a trait in the tree. `boundTo` is set when exactly one brick outside tests implements it. With several, the binding is the composition root's choice, and the workbench does not pick one |
| `external` | a type from another crate (`rusqlite::Connection`) |
| `unresolved` | a path the scan could not place, with the reason |

`std`, `core` and `alloc` are not sockets. A field holding a `String` is
data, and drawing every one would bury the parts that matter.

A generic is followed to its bound: `fn f<S: Store>(store: &S)` is a socket
into `Store`, and `S` itself is not reported as unresolved, although the
scan's own diagnostics do report it.

### Studs and grips

| grip | meaning |
|---|---|
| `brick` | another brick calls it |
| `port` | an impl's fn that no brick calls by name, but one calls the trait's fn: `SqliteActivityStore::open_session` when a use case calls `store.open_session()` |
| `route` | an entry: a `cmd_*` CLI handler or `handle_*_key` TUI handler, by the traffic view's own predicate (now `views::traffic::entry_verb`, shared so the two cannot disagree) |
| `framework` | a fn of a trait the crate does not own (`Display::fmt`, `Drop::drop`): std or another crate calls it |
| `unseen` | nothing the scan can read calls it. A lead, not a verdict |

A method in a trait impl is written without `pub` and records as private.
`ImplBinding.methods` is how a reader knows it is as public as the trait.

### Calls

`Call` edges supply calls by path: `f()`, `Type::new()`, a tuple struct or
variant built in place. A method call on a receiver made no edge before this
spec (spec 01: "a call on a receiver whose type we would have to infer is
not recorded at all"). It is now recorded raw on the symbol, as
`recv_calls`, and resolved by the workbench through what is **declared**:

1. `self.<field>`: the field's type.
2. A local, by its `let`: a type annotation, a struct literal, or a call
   to a fn in the tree. The fn's declared return type is the local's
   (`-> Self` included). This comes before the parameter, because a `let`
   shadows one.
3. A parameter: its type, or the bound of its generic.
4. A chained call, `Scanner::new(dir)?.run()`: the callee's return type.

Then the method must exist on exactly one brick behind the receiver. None is
a library call (`.clone()`, `.iter()`). Several is ambiguous and not drawn.
A local whose `let` declares nothing readable (`let x = y.compute()`) is
`unread` and not drawn.

**No `Call` edge is made from any of this.** The rules read edges, and a
rule whose answer moved because the workbench learned to read locals would
be a regression nobody asked for.

### Additions to the model

All additive, all `serde(default)` and skipped when empty, so `model.json`
for a crate with none of these shapes is unchanged and no existing reader
moves:

| where | what | why |
|---|---|---|
| `Edge.via` | the field or parameter behind a `Field`, `Sig` or `Bound` edge; `->` for a return type | every field of a struct shares the struct's line, so an edge could not say which field it came from |
| `Symbol.recv_calls` | raw receiver calls, with `ty` / `callee` resolved by the scanner's own resolver | above |
| `FnSig.generics` | each generic with its bounds | `generic_bounds` alone loses which generic a bound constrains |
| `FnSig.receiver` | `&self`, `&mut self`, `self` | a signature shown as `fn now(self)` would be a lie |
| `ImplBinding.methods` | the fns an impl block defines | above |
| `scan::scan_with_text` | the model and every parsed file's text | the source panel |

### The server

```
metatron serve [path] [--port 4477] [--no-watch]
```

`std::net` only. metatron-nestjs uses `node:http` only, for the same reason:
a workbench is no reason to take a dependency.

| route | returns |
|---|---|
| `GET /` | the page, `include_str!`'d so the binary stays single |
| `GET /api/model` | the payload, with `version`, `error`, `watching` |
| `GET /api/source?file=` | a scanned file's text, from the scan |
| `GET /api/events` | server-sent events: `{ version, error }` after each re-scan |
| `GET /api/change` | `501`, naming spec 10 |

- **It binds `127.0.0.1`**, and answers only a request whose `Host` is that
  address or `localhost`. metatron-nestjs does not check `Host`. Without
  the check, a page elsewhere could point a DNS name at the loopback address
  and read source through the browser.
- **Source comes from the scan's text map**, never from a joined path.
  `../Cargo.toml`, `%2E%2E%2F…`, absolute paths and unscanned files are 404.
- **Watching is polling**, every 250 ms, over every `.rs` file plus
  `Cargo.toml` and `metatron.toml`, skipping `target/`, dot-directories and
  `node_modules/`. A poll that sees a change waits for one quiet poll, so a
  save across several files re-scans once. The debounce counts polls rather
  than reading a clock: this crate's clock is a port, and `serve.rs` stays
  out of `[externs] time`.
- **A re-scan that fails keeps the last good model.** The page shows the
  error with the old model drawn. A first scan that fails is an error, not
  an empty bench.
- The payload is built from scan, config and classification only, without
  the git history and rules a `Scorecard` carries. A re-scan of this
  repository takes about 90 ms.

### The page

`templates/serve/workbench.html` is vendored from metatron-nestjs. Each edit
is marked `metatron-rust change` in the file:

- the highlighter reads Rust. `'` opens a char literal or a lifetime,
  never a string (upstream's TypeScript reading turned `'a` into a string
  running to the end of the line). Strings span lines, raw strings close on
  their own `#`s, and `#[…]` and `name!` take the decorator colour
- every `impl Trait for Type` with both ends on the bench is drawn as the
  dotted line, and links the port to what fills it when the neighbourhood is
  walked
- a trait is the hollow brick, a `port` grip is a teal pip, and an
  `external` socket is grey like `framework`
- the hover and legend wording is Rust's: a field holds, a parameter takes
- every brick shows its sockets section, not only classes
- a brick's subtitle names the last two module segments and is counted in
  its width. Rust module paths overflowed the brick
- the change form is hidden while the payload says `changes: false`
- `[hidden] { display:none !important; }`. Upstream, `.chg` and `.scrub`
  set `display`, which beats the `hidden` attribute, so the empty
  "change map" bar shows under the status bar whenever no change is open.
  That is a bug in metatron-nestjs too

`HIDE_BY_DEFAULT` names `patterns-rust`'s data shapes: `value-object`,
`view-model`, `projection`. They are bricks, but rarely the ones you want to
wire through.

## What it refuses

- **No inference.** A receiver is followed through what the source declares,
  or not at all. "Unseen" can therefore mean "called on a local nobody
  typed", and the page says so.
- **No binding guessed.** A port with two implementations has no `boundTo`.
- **No invented edges.** Only fields, parameters, calls and impls are drawn.
  `use` stays in the atlas.
- **No whole-crate layout.** The bench draws a neighbourhood, as upstream.

## Acceptance

1. `metatron serve` prints a `127.0.0.1` URL, and the page loads with no
   console errors and no request beyond that origin.
2. `/api/source` returns a scanned file's text, and 404 for `../Cargo.toml`,
   its percent-encoded form, an absolute path, and an unscanned path. A
   foreign `Host` is 403, a `POST` is 405.
3. A save pushes a new version over the event stream, once. A scan that
   throws leaves the previous model served, with the error.
4. On the `ports` fixture, `cmd_start` sockets into `ActivityStore` (no
   binding: two impls) and `Clock` (bound to `FixedClock`). Hovering a
   trait stud lights exactly the calls to it, and each impl's fns are
   `port` studs.
5. `tests/wiring.rs` (19) and `tests/serve.rs` (7), against a new `wiring`
   fixture holding one of each shape above.
6. Every existing test passes unchanged. The model additions move no rule.

## Results (2026-10-28)

| | bricks | sockets placed | calls | studs: brick · port · entry · trait · unseen | unread local calls | payload |
|---|---|---|---|---|---|---|
| `ports` fixture | 17 | 9 / 9 (6 port) | 13 | 8 · 7 · 0 · 0 · 9 | 1 | 16 KB |
| arioch | 25 | 25 / 25 (6 external) | 103 | 29 · 0 · 0 · 1 · 5 | 722 | 61 KB |
| enoch | 29 | 43 / 43 (12 external) | 187 | 40 · 0 · 0 · 0 · 4 | 364 | 74 KB |
| metatron | 140 | 188 / 188 (3 port, 18 external) | 403 | 76 · 1 · 0 · 10 · 27 | 991 | 199 KB |

No receiver call was ambiguous on any of the four.

**Reading locals mattered more than in NestJS.** The first cut followed
fields and parameters only, as upstream follows `this.x`. On metatron it
left 39 unseen studs, and most were false. `Scorecard::exit_code`,
`Workbench::meta` and `Narration::paragraphs` are all called on a local
(`let s = scorecard::build(..)?`), and `Scanner::run` on a chained call. In
a Nest backend almost every call goes through an injected `this.x`. In Rust
without an injector, most go through locals. Following the `let` took
metatron from 315 calls to 403 and from 39 unseen studs to 27. Of the 27,
the test-only API (`assert_*`, `check`, `Workbench::close`) is called
from outside the scanned tree. Most of the rest are `pub` fns called only
inside their own type or module (`Finding::gates` through `self` or a
closure). Both are what `unseen` is for.

`unread` is large and mostly library calls on iterators and strings. It is
in the payload's `wiringMeta`, not on the bench.

Checked in headless Chromium over the DevTools protocol, as upstream's
README says the workbench must be (the event stream never lets a
`--screenshot` run go idle). Light and dark, on the fixture, arioch and this
repository: no console errors, and hover lights exactly the calls in
`calls`.

## Out of scope

- **The change overlay** (metatron-nestjs spec 11). It needs spec 10's two
  scans of a range. The server answers `/api/change` with 501 so the gap is
  stated rather than silent.
- **Generic struct fields.** `struct App<S: Store> { store: S }` keeps no
  generics on the struct, so `store` is not a socket into `Store`.
- **Methods in another file.** A type's lines span its own file. An `impl`
  in a second file still gives its studs, but they cannot be flashed in the
  source panel.
- **Closure parameters**, destructured `let`s, and a local bound to a
  method's result. Each would need inference or more parsing, and is counted
  as `unread`.
- **Layout.** A crate whose symbols mostly sit on one layer, or on
  Unclassified, draws a wide single row whose edges loop. That is upstream's
  layout faithfully drawing a flat classification. The fix is the
  classification.
