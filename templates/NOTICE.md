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
