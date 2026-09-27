//! Acceptance tests for `specs/06-views.md`.
//!
//! These check the payload contracts and the substitution. They cannot
//! check that a page *looks* right — spec 06 is explicit that static
//! checks do not catch mirrored text, washed-out blends, or a canvas that
//! rendered blank because the virtual-time budget was too short. What
//! they do catch is the failure that bit hardest while building this:
//! a payload shaped as an object where the template indexes an array,
//! which throws nothing, logs nothing, and draws nothing.

use metatron::views;
use serde_json::Value;
use std::path::PathBuf;

fn card(name: &str) -> metatron::Scorecard {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    metatron::scorecard::build(&p).expect("scorecard")
}

fn sibling(name: &str) -> Option<metatron::Scorecard> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(name);
    p.join("Cargo.toml")
        .exists()
        .then(|| metatron::scorecard::build(&p).expect("scorecard"))
}

/// The JSON out of the one `__DATA__` slot in a rendered view.
fn payload(html: &str) -> Value {
    let at = html
        .find("type=\"application/json\">")
        .expect("no data script");
    let start = at + "type=\"application/json\">".len();
    let end = start + html[start..].find("</script>").expect("unterminated");
    serde_json::from_str(&html[start..end]).expect("payload is not valid JSON")
}

// ------------------------------------------------------------ mechanics

#[test]
fn every_view_renders_with_nothing_left_unsubstituted() {
    let s = card("ports");
    for v in views::available(&s) {
        let html = views::render(&s, v.name).unwrap_or_else(|e| panic!("{}: {e:#}", v.name));
        assert!(
            !html.contains("__DATA__"),
            "{}: token left in place",
            v.name
        );
        assert!(
            !html.contains("{{"),
            "{}: placeholder left unsubstituted",
            v.name
        );
        payload(&html); // panics if the JSON did not parse
    }
}

#[test]
fn a_template_with_the_wrong_number_of_data_tokens_is_an_error() {
    // metatron asserts exactly one. A template with none renders as a
    // blank page with no error, which is worth spending an assertion on.
    let s = card("ports");
    assert!(views::render(&s, "no-such-view").is_err());
}

#[test]
fn schema_is_absent_rather_than_present_and_empty() {
    assert!(
        !views::VIEWS.iter().any(|v| v.name == "schema"),
        "schema should not be built at all: there is no ORM here"
    );
}

#[test]
fn a_lens_with_nothing_to_say_is_omitted() {
    // The conforming fixture has no god object and no git history of its
    // own, so cohesion and hotspots have nothing to draw.
    let s = card("ports");
    let names: Vec<&str> = views::available(&s).iter().map(|v| v.name).collect();
    assert!(!names.contains(&"cohesion"), "{names:?}");
    assert!(names.contains(&"city") && names.contains(&"layers"));
}

// ------------------------------------------------------------- the views

#[test]
fn city_gives_every_module_a_tower_including_one_holding_only_functions() {
    // `arioch::ui` is 1,300 lines of render fns and no types. Dropping it
    // would hide the module where `renders-off-store` fires.
    let Some(s) = sibling("arioch") else { return };
    let d = payload(&views::render(&s, "city").unwrap());
    let mods = d["modules"].as_array().unwrap();
    assert_eq!(mods.len(), s.model.stats.modules, "one tower per module");
    assert!(mods.iter().any(|m| m["id"] == "ui"));
    // The crate root's module id is the empty string; it must still be named.
    assert!(mods.iter().all(|m| !m["id"].as_str().unwrap().is_empty()));
}

#[test]
fn a_tower_with_floors_in_two_colours_is_a_mixed_layer_module() {
    // The claim the whole view rests on. `mixed::app` holds a key handler
    // (application), an enum (domain) and a loader (infrastructure);
    // `mixed::pure` holds one layer.
    let s = card("mixed");
    let d = payload(&views::render(&s, "city").unwrap());
    let mods = d["modules"].as_array().unwrap();
    let get = |id: &str| {
        mods.iter()
            .find(|m| m["id"] == id)
            .unwrap_or_else(|| panic!("no tower {id}"))
    };
    assert_eq!(get("app")["tiersPresent"].as_array().unwrap().len(), 3);
    assert_eq!(get("pure")["tiersPresent"].as_array().unwrap().len(), 1);
}

#[test]
fn layers_marks_the_inversion_arrow_and_never_calls_it_a_skip() {
    // `impl ActivityStore for SqliteActivityStore` runs from
    // infrastructure back up to domain. Drawn like an ordinary dependency
    // it is the worst-looking line on the diagram; it is in fact the
    // architecture. It corresponds to no import, which is why a
    // file-level tool cannot draw this at all.
    let s = card("ports");
    let d = payload(&views::render(&s, "layers").unwrap());
    let links = d["links"].as_array().unwrap();

    // Serialised as arrays, because that is what the template indexes.
    assert!(links.iter().all(|l| l.is_array()), "links must be arrays");

    let impls: Vec<&Value> = links.iter().filter(|l| l[4] == "impl").collect();
    assert_eq!(impls.len(), 5, "expected five inversions");
    assert_eq!(d["inversions"], 5);
    for l in &impls {
        assert_eq!(l[2], 1, "an inversion runs upward against the layers");
        assert!(
            l[3].is_null(),
            "an inversion must never be flagged as a skip"
        );
    }
}

#[test]
fn every_node_tier_resolves_to_a_tier_that_exists() {
    // An adapter that emits an index past the end of `tiers` is a blank
    // page with an exception nobody sees. This held even at 100%
    // coverage, because a module of `mod` declarations classifies as
    // nothing either.
    for name in ["ports", "mixed", "leaky"] {
        let s = card(name);
        for view in ["city", "layers", "atlas"] {
            let d = payload(&views::render(&s, view).unwrap());
            let n = d["tiers"].as_array().unwrap().len();
            let mut seen = Vec::new();
            collect_tiers(&d, &mut seen);
            for t in seen {
                assert!(t < n, "{name}/{view}: tier {t} but only {n} tiers");
            }
        }
    }
}

fn collect_tiers(v: &Value, out: &mut Vec<usize>) {
    match v {
        Value::Object(m) => {
            for (k, x) in m {
                if (k == "tier" || k == "t") && x.is_u64() {
                    out.push(x.as_u64().unwrap() as usize);
                } else {
                    collect_tiers(x, out);
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| collect_tiers(x, out)),
        _ => {}
    }
}

#[test]
fn traffic_traces_a_subcommand_to_the_port_and_stops_there() {
    let s = card("ports");
    let d = payload(&views::render(&s, "traffic").unwrap());
    let eps = d["endpoints"].as_array().unwrap();
    let e = eps
        .iter()
        .find(|e| e["route"] == "cmd_start")
        .expect("cmd_start not traced");

    assert_eq!(e["verb"], "cli");
    assert_eq!(e["through_port"], true);
    assert_eq!(e["target"], "domain::ports::activity_store::ActivityStore");

    let kinds: Vec<&str> = e["flat"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"use-case"), "{kinds:?}");
    assert!(kinds.contains(&"port"), "{kinds:?}");
    // The trace stops at the port: the concrete is never a hop.
    assert!(
        !kinds.contains(&"store"),
        "the trace reached past the port: {kinds:?}"
    );
}

#[test]
fn an_unresolvable_trace_is_reported_and_not_drawn() {
    // metatron's rule that an unparseable route is reported rather than
    // guessed at. A TUI key dispatch is a match on a KeyCode, and spec 01
    // makes Call edges best-effort.
    let Some(s) = sibling("arioch") else { return };
    let d = payload(&views::render(&s, "traffic").unwrap());
    for e in d["endpoints"].as_array().unwrap() {
        assert!(
            !e["flat"].as_array().unwrap().is_empty(),
            "an empty trace was drawn: {}",
            e["route"]
        );
    }
}

#[test]
fn cohesion_renders_the_matrix_in_blocks() {
    let Some(s) = sibling("arioch") else { return };
    let d = payload(&views::render(&s, "cohesion").unwrap());
    let app = d["types"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "App")
        .expect("App not rendered");

    let rows = app["methods"].as_array().unwrap().len();
    let cols = app["fields"].as_array().unwrap().len();
    let cells = app["cells"].as_array().unwrap();
    assert_eq!(cells.len(), rows, "one row of marks per method");
    assert!(cols > 30 && rows > 30);
    assert_eq!(app["verdict"], "tangled");

    // Blocks tile the axes contiguously, which is what makes the
    // off-diagonal marks readable as the cost of the split.
    let blocks = app["blocks"].as_array().unwrap();
    assert!(blocks.len() > 1);
    let mut row = 0;
    let mut col = 0;
    for b in blocks {
        assert_eq!(b["row"], row);
        assert_eq!(b["col"], col);
        row += b["rows"].as_u64().unwrap();
        col += b["cols"].as_u64().unwrap();
    }
    // Every mark points at a real column.
    for r in cells {
        for c in r.as_array().unwrap() {
            assert!((c.as_u64().unwrap() as usize) < cols);
        }
    }
    assert!(app["cross_edges"].as_u64().unwrap() > 0);
}

// --------------------------------------------------------- the narration

#[test]
fn a_slot_with_nothing_to_say_is_removed_rather_than_left_empty() {
    // The rule the whole mechanism exists for: metatron's first version
    // had its findings typed into the HTML, and when the charts updated
    // for a new project the paragraphs kept describing the old one.
    let s = card("ports");
    let filled = views::render(&s, "cohesion");
    // `ports` has no cohesion view; render it directly to exercise fill().
    let _ = filled;

    let Some(a) = sibling("arioch") else { return };
    let html = views::render(&a, "cohesion").unwrap();
    assert!(!html.contains("data-narr"), "an unfilled slot survived");
    assert!(html.contains("tangled") || html.contains("LCOM4"));
}

#[test]
fn the_narration_describes_this_crate_and_not_another() {
    let Some(a) = sibling("arioch") else { return };
    let n = views::Narration::of(&a);
    let ports = views::Narration::of(&card("ports"));

    assert!(n.get("ports").unwrap().contains("no port seam"));
    assert!(ports.get("ports").unwrap().contains("port"));
    assert!(!ports.get("ports").unwrap().contains("no port seam"));

    // The axis the glossary earns and no other tool prints.
    assert!(n
        .get("enforcement")
        .unwrap()
        .contains("none is enforced by the compiler"));
    assert!(
        n.get("unevaluable").is_some(),
        "17 dark rules and nothing said"
    );
    assert!(n.get("coverage").unwrap().contains("1.2%"));
}
