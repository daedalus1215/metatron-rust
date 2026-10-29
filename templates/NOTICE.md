# Vendored templates

`city.html`, `layers.html`, `traffic.html`, `atlas.html` and `hotspots.html`
are copied unmodified from `metatron-nestjs/templates/`, except where an
implementation note in `specs/06-views.md` records a change.

They are embedded with `include_str!` rather than shipped as a directory,
so `metatron` stays a single binary. Each contains exactly one `__DATA__`
token in a JSON `<script>` tag plus `{{project}}`-style placeholders; the
build substitutes both.

`schema.html` is deliberately not vendored — see spec 06, "schema — defer".
`cohesion.html` has no upstream original and is written here.

`serve/workbench.html` is copied from `metatron-nestjs/templates/serve/` and
served by `metatron serve` rather than built (spec 11). Every edit is marked
`metatron-rust change` in the file and listed in spec 11, "The page": the Rust
highlighter, `impl` lines, the trait as the hollow brick, the `port` grip and
`external` socket, Rust wording, sockets on every brick, subtitle widths, the
hidden change form, and a `[hidden]` rule that is a fix for upstream too.
