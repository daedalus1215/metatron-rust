# render-check

Loads every rendered view in jsdom, runs its scripts, and **counts real
drawing calls**.

```
npm install --no-save jsdom canvas
metatron views . --out /tmp/v
node tools/render-check.mjs /tmp/v
```

The draw counter is the point. `tests/views.rs` checks the payload
contracts, and jsdom on its own reports a page as fine when the script
ran without throwing. But a payload shaped as an object where the
template indexes an array — `l[0]` on `{a, b}` is `undefined` — throws
nothing, logs nothing, and draws nothing. That bug shipped through both
the static checks and a clean jsdom run, and only a stroke count caught
it.

It still does not check that a page *looks* right. Per spec 06, that
needs eyes on a screenshot: mirrored text, washed-out blends and a canvas
that rendered blank because the virtual-time budget was too short all
survive every check here.
