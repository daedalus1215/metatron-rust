# render-check

Loads every rendered view in jsdom, runs its scripts, and **counts real
drawing calls**.

```
npm --prefix tools install
metatron views . --out /tmp/v
node tools/render-check.mjs /tmp/v
```

or, in one line from the repository root:

```
npm --prefix tools install && metatron views . --out /tmp/v && node tools/render-check.mjs /tmp/v
```

`tools/package.json` carries the two dependencies this needs — `jsdom` and
`canvas` — so the check runs the same way on a fresh clone as it does on a
machine that has run it before. The old instructions installed them with
`npm install --no-save jsdom canvas` in the repository root, which put a
`node_modules/` beside `Cargo.toml` and left nothing behind to say what the
check needed.

The draw counter is the point. `tests/views.rs` checks the payload
contracts, and jsdom on its own reports a page as fine when the script
ran without throwing. But a payload shaped as an object where the
template indexes an array — `l[0]` on `{a, b}` is `undefined` — throws
nothing, logs nothing, and draws nothing. That bug shipped through both
the static checks and a clean jsdom run, and only a stroke count caught
it.

**`canvas` is a native module, and it does not always load.** When it does
not, jsdom answers `getContext` with `Not implemented`, and `city` and
`layers` — the two views that draw — are reported `ERROR` with that message.
That is the environment, not the page: the other four views still pass, and
the two failures name their own reason rather than counting as a clean run.
When a prebuilt binary is refused (`ELF load command address/offset not
page-aligned` and similar), the fix is a working system `libcairo` and a
rebuild from source, not a change to these templates.

It still does not check that a page *looks* right. Per spec 06, that
needs eyes on a screenshot: mirrored text, washed-out blends and a canvas
that rendered blank because the virtual-time budget was too short all
survive every check here.
