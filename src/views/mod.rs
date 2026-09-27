//! Rendering the model into metatron's lenses. See `specs/06-views.md`.
//!
//! The templates are `include_str!`'d rather than shipped as a directory,
//! so `metatron` stays a single binary. Each holds exactly one `__DATA__`
//! token in a JSON `<script>` tag plus `{{project}}`-style placeholders.
//!
//! What changes from metatron-nestjs is the *payload*, because the model
//! underneath is symbols rather than files. What does not change is the
//! rule the whole `narrate` mechanism exists to enforce: **never type a
//! finding into a template.** metatron's first version had its findings
//! typed into the HTML, and when the charts updated for a new project the
//! paragraphs kept confidently describing the old one.

mod atlas;
mod city;
mod cohesion;
mod hotspots;
mod layers;
mod narrate;
mod traffic;

pub use narrate::Narration;

use crate::scorecard::Scorecard;
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::path::Path;

pub struct View {
    pub name: &'static str,
    pub title: &'static str,
    pub blurb: &'static str,
    template: &'static str,
}

/// `schema` is absent rather than present and empty: metatron's build
/// drops narration slots with nothing to say, and the same instinct
/// applies to a whole lens. There is no ORM here, and neither target
/// codebase has a domain layer to draw yet.
pub const VIEWS: &[View] = &[
    View {
        name: "atlas",
        title: "Atlas",
        blurb: "The module map and the conformance ledger.",
        template: include_str!("../../templates/atlas.html"),
    },
    View {
        name: "city",
        title: "City",
        blurb: "One tower per module, one floor per type, coloured by layer.",
        template: include_str!("../../templates/city.html"),
    },
    View {
        name: "layers",
        title: "Layers",
        blurb: "Symbols on the plane of their layer, with the inversion arrow drawn apart.",
        template: include_str!("../../templates/layers.html"),
    },
    View {
        name: "traffic",
        title: "Traffic",
        blurb: "A CLI subcommand or TUI key traced through to the port.",
        template: include_str!("../../templates/traffic.html"),
    },
    View {
        name: "hotspots",
        title: "Hotspots",
        blurb: "Git churn against dependents.",
        template: include_str!("../../templates/hotspots.html"),
    },
    View {
        name: "cohesion",
        title: "Cohesion",
        blurb: "Method-by-field access, ordered so components fall into blocks.",
        template: include_str!("../../templates/cohesion.html"),
    },
];

/// Fields every payload carries, so the templates' shared chrome works.
#[derive(Serialize)]
pub struct Common<'a> {
    pub generated_at: String,
    pub project: &'a str,
    pub root: &'a str,
    pub stats: Stats,
    pub tiers: Vec<Tier>,
}

/// metatron's tier index is a NestJS concept (Entry .. Wiring). Here the
/// ordered layers from `metatron.toml` fill the same slot, which is what
/// lets the templates render unchanged.
#[derive(Serialize, Clone)]
pub struct Tier {
    pub i: usize,
    pub name: String,
    pub sub: String,
    /// Hidden unless the viewer asks for it: the test layer, and the
    /// plane for everything the config does not recognise.
    #[serde(default)]
    pub extra: bool,
}

#[derive(Serialize, Clone, Copy)]
pub struct Stats {
    pub files: usize,
    pub modules: usize,
    pub symbols: usize,
    pub types: usize,
    pub fns: usize,
    pub edges: usize,
    pub loc: u32,
    pub coverage: f64,
}

/// The ordered layers, **plus a trailing plane for everything the config
/// does not recognise** whenever the crate has any.
///
/// Adapters index unclassified symbols at `layers.len()`, and every
/// template does `tiers[node.tier].name` without checking. Against a
/// crate at 1% coverage that is most of the nodes, so omitting this tier
/// is not a cosmetic gap — the page throws on the first module and
/// renders blank. Drawing the unclassified plane is also the right call
/// on its own: a symbol the classifier missed is the one most worth
/// seeing.
pub fn tiers(s: &Scorecard) -> Vec<Tier> {
    let mut v: Vec<Tier> = s
        .config
        .layers
        .iter()
        .enumerate()
        .map(|(i, l)| Tier {
            i,
            name: l.title.clone(),
            sub: l.sub.clone(),
            extra: l.id == "test",
        })
        .collect();
    // Always appended, even at 100% coverage. A module holding only `mod`
    // declarations has no classified symbol either, so the index is
    // reachable on a fully conforming crate too — and an adapter that can
    // emit an index the template cannot resolve is a blank page with no
    // error in the console.
    v.push(Tier {
        i: v.len(),
        name: "Unclassified".into(),
        sub: "matched no pattern in metatron.toml".into(),
        extra: false,
    });
    v
}

pub fn tier_index(s: &Scorecard, layer: &str) -> Option<usize> {
    s.config.layers.iter().position(|l| l.id == layer)
}

/// The crate root's module id is the empty string. Left as-is it draws a
/// nameless tower and an unlabelled band.
pub fn mod_label(id: &str, project: &str) -> String {
    if id.is_empty() {
        format!("{project} (crate root)")
    } else {
        id.to_string()
    }
}

pub fn stats(s: &Scorecard) -> Stats {
    let t = &s.model.stats;
    Stats {
        files: t.files,
        modules: t.modules,
        symbols: t.symbols,
        types: t.types,
        fns: t.fns,
        edges: t.edges,
        loc: t.loc,
        coverage: s.coverage(),
    }
}

fn payload(s: &Scorecard, name: &str) -> Result<String> {
    Ok(match name {
        "atlas" => serde_json::to_string(&atlas::build(s))?,
        "city" => serde_json::to_string(&city::build(s))?,
        "layers" => serde_json::to_string(&layers::build(s))?,
        "traffic" => serde_json::to_string(&traffic::build(s))?,
        "hotspots" => serde_json::to_string(&hotspots::build(s)?)?,
        "cohesion" => serde_json::to_string(&cohesion::build(s))?,
        _ => bail!("unknown view `{name}`"),
    })
}

/// Render one view to HTML.
pub fn render(s: &Scorecard, name: &str) -> Result<String> {
    let v = VIEWS
        .iter()
        .find(|v| v.name == name)
        .with_context(|| format!("unknown view `{name}`"))?;

    // metatron's build asserts exactly one token. A template with none
    // would render as a blank page with no error, which is the failure
    // mode worth spending an assertion on.
    let hits = v.template.matches("__DATA__").count();
    if hits != 1 {
        bail!("template `{name}` has {hits} __DATA__ tokens, expected exactly 1");
    }

    let json = payload(s, name)?;
    let pretty = s
        .model
        .project
        .replace(['-', '_'], " ")
        .split(' ')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    let html = v
        .template
        .replace("__DATA__", &json)
        .replace("{{project}}", &pretty)
        .replace("{{projectId}}", &s.model.project)
        .replace("{{date}}", &crate::baseline::today())
        .replace("{{root}}", &s.config.root)
        .replace("{{files}}", &s.model.stats.files.to_string())
        .replace("{{modules}}", &s.model.stats.modules.to_string())
        .replace("{{imports}}", &s.model.stats.edges.to_string())
        .replace("{{symbols}}", &s.model.stats.symbols.to_string())
        .replace("{{endpoints}}", &traffic::entry_count(s).to_string());

    Ok(narrate::fill(&html, s))
}

/// Which views have something to say about this crate. A lens with an
/// empty payload is omitted, not emptied.
pub fn available(s: &Scorecard) -> Vec<&'static View> {
    VIEWS
        .iter()
        .filter(|v| match v.name {
            "cohesion" => !s.cohesion_flags().is_empty(),
            "traffic" => traffic::entry_count(s) > 0,
            "hotspots" => hotspots::has_history(&s.dir, &s.config.root),
            _ => true,
        })
        .collect()
}

pub fn write_all(s: &Scorecard, out: &Path) -> Result<Vec<String>> {
    std::fs::create_dir_all(out)?;
    let mut written = Vec::new();
    for v in available(s) {
        let html = render(s, v.name)?;
        let p = out.join(format!("{}.html", v.name));
        std::fs::write(&p, html).with_context(|| format!("writing {}", p.display()))?;
        written.push(v.name.to_string());
    }
    std::fs::write(out.join("index.html"), index(s, &written))?;
    Ok(written)
}

fn index(s: &Scorecard, written: &[String]) -> String {
    let n = narrate::Narration::of(s);
    let cards: String = VIEWS
        .iter()
        .filter(|v| written.iter().any(|w| w == v.name))
        .map(|v| {
            format!(
                "<a class=v href=\"{0}.html\"><h2>{1}</h2><p>{2}</p></a>",
                v.name, v.title, v.blurb
            )
        })
        .collect();
    let omitted: Vec<&str> = VIEWS
        .iter()
        .filter(|v| !written.iter().any(|w| w == v.name))
        .map(|v| v.name)
        .collect();
    // The schema note is unconditional: a lens deliberately not built is
    // worth saying out loud, or its absence reads as an oversight.
    let omitted_html = if omitted.is_empty() {
        "<p class=om>omitted: <b>schema</b> — no ORM in Rust, and neither target \
         codebase has a domain layer to draw yet.</p>"
            .to_string()
    } else {
        format!(
            "<p class=om>omitted, having nothing to say about this crate: <b>{}</b>. \
             Also <b>schema</b> — no ORM in Rust, and neither target codebase has a \
             domain layer to draw yet.</p>",
            omitted.join("</b>, <b>")
        )
    };
    format!(
        r#"<!doctype html><meta charset=utf-8><title>{proj} · metatron</title>
<style>
:root{{color-scheme:light dark;--bg:#faf9f7;--fg:#1a1a1a;--mut:#6b6b6b;--line:#e0ddd8;--card:#fff}}
@media(prefers-color-scheme:dark){{:root{{--bg:#16161a;--fg:#e8e6e3;--mut:#9a9a9a;--line:#2c2c33;--card:#1e1e24}}}}
*{{box-sizing:border-box}}
body{{margin:0;padding:3rem 1.5rem;background:var(--bg);color:var(--fg);
font:15px/1.6 ui-sans-serif,system-ui,-apple-system,sans-serif}}
main{{max-width:60rem;margin:0 auto}}
h1{{font-size:1.6rem;margin:0 0 .2rem}}
.sub{{color:var(--mut);margin:0 0 2rem}}
.sc{{display:grid;grid-template-columns:repeat(auto-fit,minmax(9rem,1fr));gap:.75rem;margin:0 0 2rem}}
.sc div{{background:var(--card);border:1px solid var(--line);border-radius:.5rem;padding:.75rem .9rem}}
.sc b{{display:block;font-size:1.5rem;font-variant-numeric:tabular-nums}}
.sc span{{color:var(--mut);font-size:.78rem;text-transform:uppercase;letter-spacing:.04em}}
.g{{display:grid;grid-template-columns:repeat(auto-fit,minmax(15rem,1fr));gap:1rem}}
.v{{display:block;background:var(--card);border:1px solid var(--line);border-radius:.6rem;
padding:1.1rem 1.2rem;text-decoration:none;color:inherit}}
.v:hover{{border-color:var(--mut)}}
.v h2{{font-size:1rem;margin:0 0 .3rem}}
.v p{{margin:0;color:var(--mut);font-size:.86rem}}
.narr{{background:var(--card);border:1px solid var(--line);border-radius:.6rem;
padding:1.1rem 1.3rem;margin:0 0 2rem}}
.narr p{{margin:0 0 .7rem}}.narr p:last-child{{margin:0}}
.om{{color:var(--mut);font-size:.84rem;margin:2rem 0 0}}
</style>
<main>
<h1>{proj}</h1>
<p class=sub>{date} · {loc} lines · {modules} modules · {symbols} symbols</p>
<div class=sc>
<div><b>{cov:.1}%</b><span>coverage</span></div>
<div><b>{ports}</b><span>ports</span></div>
<div><b>{viol}</b><span>violations</span></div>
<div><b>{unev}</b><span>unevaluable</span></div>
<div><b>{comp}</b><span>compiler-enforced</span></div>
</div>
<div class=narr>{narr}</div>
<div class=g>{cards}</div>
{omitted}
</main>"#,
        proj = s.model.project,
        date = crate::baseline::today(),
        loc = s.model.stats.loc,
        modules = s.model.stats.modules,
        symbols = s.model.stats.symbols,
        cov = s.coverage(),
        ports = s.classified.coverage.ports,
        viol = s.diff.new.len() + s.diff.known.len(),
        unev = s.counts().unevaluable,
        comp = s.enforcement().compiler,
        narr = n
            .paragraphs()
            .iter()
            .map(|p| format!("<p>{p}</p>"))
            .collect::<String>(),
        cards = cards,
        omitted = omitted_html,
    )
}
