//! Acceptance tests for `specs/02-cohesion-analysis.md`.

use metatron::cohesion::{analyse, CohesionReport, TypeCohesion, Verdict};
use metatron::model::Model;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn report_of(dir: PathBuf) -> CohesionReport {
    analyse(&metatron::scan(&dir).expect("scan failed"))
}

fn fixture() -> CohesionReport {
    report_of(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cohesion"))
}

/// The real checkouts live beside this one. Absent, these tests pass
/// vacuously rather than failing on someone else's machine.
fn sibling(name: &str) -> Option<Model> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(name);
    p.join("Cargo.toml")
        .exists()
        .then(|| metatron::scan(&p).ok())?
}

fn ty<'a>(r: &'a CohesionReport, name: &str) -> &'a TypeCohesion {
    r.types
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("no cohesion entry for {name}"))
}

/// The partition, as a set of field-sets — comparable across renames.
fn shape(t: &TypeCohesion) -> BTreeSet<Vec<String>> {
    t.components
        .iter()
        .map(|c| {
            let mut f = c.fields.clone();
            f.sort();
            f
        })
        .collect()
}

// ------------------------------------------------------ the naming test

#[test]
fn the_partition_survives_destroying_every_name() {
    // `Anon` is `Split` with `a1..b3` rewritten to `f1..f6` and the method
    // names to `m1..m5`. Same access structure, so the same partition —
    // otherwise the detector is a naming lint wearing a metric's clothes.
    let r = fixture();
    let split = ty(&r, "Split");
    let anon = ty(&r, "Anon");

    assert_eq!(split.lcom4, 2);
    assert_eq!(anon.lcom4, split.lcom4);
    assert_eq!(anon.modularity, split.modularity);
    assert_eq!(anon.components.len(), split.components.len());

    let sizes = |t: &TypeCohesion| {
        let mut v: Vec<_> = t
            .components
            .iter()
            .map(|c| (c.fields.len(), c.methods.len()))
            .collect();
        v.sort();
        v
    };
    assert_eq!(sizes(anon), sizes(split));
}

#[test]
fn the_same_holds_for_the_real_god_object() {
    // The fixture proves it on six fields. This proves it on forty-one,
    // by renaming `App` in the model and re-running.
    let Some(m) = sibling("arioch") else { return };
    let before = analyse(&m);
    let Some(app) = before.types.iter().find(|t| t.symbol == "app::App") else {
        return;
    };

    let fmap: Vec<(String, String)> = m
        .symbol("app::App")
        .unwrap()
        .fields
        .iter()
        .enumerate()
        .map(|(i, f)| (f.name.clone(), format!("f{i}")))
        .collect();
    let rename = |n: &str| {
        fmap.iter()
            .find(|(o, _)| o == n)
            .map(|(_, x)| x.clone())
            .unwrap_or_else(|| n.to_string())
    };

    let mut m2 = m.clone();
    for s in m2.symbols.iter_mut() {
        if s.id == "app::App" {
            for f in s.fields.iter_mut() {
                f.name = rename(&f.name);
            }
        }
        if s.parent.as_deref() == Some("app::App") {
            for v in s.self_fields.iter_mut().chain(s.self_reads.iter_mut()) {
                *v = rename(v);
            }
        }
    }
    let after = analyse(&m2);
    let app2 = after
        .types
        .iter()
        .find(|t| t.symbol == "app::App")
        .expect("App after rename");

    assert_eq!(app2.lcom4, app.lcom4);
    assert_eq!(app2.modularity, app.modularity);

    // Map the renamed partition back and compare it to the original.
    let back: BTreeSet<Vec<String>> = shape(app2)
        .into_iter()
        .map(|c| {
            let mut v: Vec<String> = c
                .iter()
                .map(|n| {
                    fmap.iter()
                        .find(|(_, x)| x == n)
                        .map(|(o, _)| o.clone())
                        .unwrap_or_else(|| n.clone())
                })
                .collect();
            v.sort();
            v
        })
        .collect();
    assert_eq!(back, shape(app), "partition moved when only names changed");
}

// -------------------------------------------------------- false positives

#[test]
fn a_stateless_private_helper_does_not_split_its_callers() {
    // `Helper::norm` touches no field. `one` and `two` both call it, and
    // are joined through it. A live-subgraph filter that dropped `norm`
    // for having degree zero would report two components here.
    let r = fixture();
    let t = ty(&r, "Helper");
    assert_eq!(t.lcom4, 1, "stateless helper split the type");
    assert_eq!(t.verdict, Verdict::Cohesive);
}

#[test]
fn a_record_with_no_methods_is_not_analysed() {
    // Every field of a settings struct is untouched by `self`, because it
    // has no methods. Reporting that is how a tool trains people to ignore
    // it. `arioch::Entry`, `DialogState`, `CategoryColors` are records.
    let Some(m) = sibling("arioch") else { return };
    let r = analyse(&m);
    for name in ["Entry", "DialogState", "CategoryColors", "KnowledgeEntry"] {
        assert!(
            !r.types.iter().any(|t| t.name == name),
            "{name} is a record and should not be in the report"
        );
    }
}

#[test]
fn an_enum_is_not_analysed() {
    // Variant fields are disjoint by construction, so LCOM4 over them
    // recounts the variants and calls every enum maximally incohesive.
    let Some(m) = sibling("arioch") else { return };
    let r = analyse(&m);
    assert!(!r
        .types
        .iter()
        .any(|t| t.name == "Command" || t.name == "Mode"));
}

#[test]
fn a_pub_field_read_from_another_module_is_not_dead() {
    // `App::selected_category` is never touched through `self` by any of
    // App's methods, and is read at `ui.rs` as `app.selected_category` —
    // inside a `format!`, where syn does not look without help.
    let Some(m) = sibling("arioch") else { return };
    let r = analyse(&m);
    let app = ty(&r, "App");
    assert!(
        !app.unused_fields.contains(&"selected_category".to_string()),
        "claimed a field is dead that another module reads: {:?}",
        app.unused_fields
    );
}

// ------------------------------------------------------------ true positives

#[test]
fn a_field_written_and_never_read_is_reported() {
    let r = fixture();
    let t = ty(&r, "Counter");
    assert_eq!(t.write_only_fields, vec!["tally".to_string()]);
    // `total` is assigned in `bump` and read in `report`.
    assert!(!t.write_only_fields.contains(&"total".to_string()));
    assert!(
        t.unused_fields.is_empty(),
        "a written field is not untouched"
    );
}

#[test]
fn a_type_with_macro_generated_methods_makes_no_claims() {
    let r = fixture();
    let t = ty(&r, "Tainted");
    assert_eq!(t.verdict, Verdict::Excluded);
    assert!(t.components.is_empty());
    // `p` and `q` are touched only by the generated method, so a partial
    // analysis would report them dead.
    assert!(t.unused_fields.is_empty(), "{:?}", t.unused_fields);
    assert!(t.write_only_fields.is_empty());
}

#[test]
fn disjoint_responsibilities_report_as_disconnected() {
    let r = fixture();
    let t = ty(&r, "Split");
    assert_eq!(t.verdict, Verdict::Disconnected);
    assert_eq!(t.components.len(), 2);
    for c in &t.components {
        let heads: BTreeSet<char> = c.fields.iter().filter_map(|f| f.chars().next()).collect();
        assert_eq!(
            heads.len(),
            1,
            "a component mixed the two groups: {:?}",
            c.fields
        );
    }
}

// ------------------------------------------------------------- calibration

#[test]
fn a_store_with_one_connection_does_not_decompose() {
    // `enoch::db::Db` is 24 methods over a single `conn`. That is a store
    // doing a store's job. A detector that splits it is measuring size.
    let Some(m) = sibling("enoch") else { return };
    let r = analyse(&m);
    let Some(db) = r.types.iter().find(|t| t.name == "Db") else {
        return;
    };
    assert_eq!(db.verdict, Verdict::Cohesive, "split a legitimate store");
}

#[test]
fn ariochs_most_reasonable_type_is_not_called_a_god_object() {
    let Some(m) = sibling("arioch") else { return };
    let r = analyse(&m);
    if let Some(reg) = r.types.iter().find(|t| t.name == "Registry") {
        assert_eq!(reg.verdict, Verdict::Cohesive);
    }
}

#[test]
fn the_god_object_is_tangled_rather_than_splittable() {
    // 41 fields and 46 methods, and no seam: the components that exist
    // share eleven fields. This is the worse of the two findings — nothing
    // lifts out without dragging shared state along.
    let Some(m) = sibling("arioch") else { return };
    let r = analyse(&m);
    let app = ty(&r, "App");
    assert_eq!(app.field_count, 41);
    assert_eq!(app.verdict, Verdict::Tangled);
    assert!(app.modularity < 0.30, "Q={}", app.modularity);
    assert!(app.components.len() > 1);
    assert!(app.shared.len() >= 5, "{:?}", app.shared);
    assert!(app.cross_edges > 0);
}

#[test]
fn related_state_lands_in_one_component() {
    // The four families named in spec 02. Each must be undivided — and
    // this is decided by access alone; the prefixes are only how the test
    // states the expectation.
    let Some(m) = sibling("arioch") else { return };
    let r = analyse(&m);
    let app = ty(&r, "App");
    for prefix in [
        "suggestion_",
        "annot_",
        "map_",
        "investigate_",
        "search_",
        "bulk_",
    ] {
        let hit: BTreeSet<usize> = app
            .components
            .iter()
            .enumerate()
            .filter(|(_, c)| c.fields.iter().any(|f| f.starts_with(prefix)))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(hit.len(), 1, "{prefix}* was split across {hit:?}");
    }
}

// ------------------------------------------------------- mixed concerns

#[test]
fn a_function_that_reads_the_disk_and_draws_is_reported() {
    let r = fixture();
    let f = r
        .mixed_concern
        .iter()
        .find(|f| f.symbol.ends_with("render_file"))
        .expect("render_file not reported");
    let cs: Vec<&str> = f.concerns.keys().map(String::as_str).collect();
    assert_eq!(cs, vec!["io", "ui"]);
    // The control: same module, same imports, one concern.
    assert!(!r
        .mixed_concern
        .iter()
        .any(|f| f.symbol.ends_with("render_blank")));
}

#[test]
fn std_is_grouped_by_module_not_by_crate() {
    // `std` spans every concern there is, so keying the table on the crate
    // name puts `std::fs` and `std::process` in the same bucket as
    // `std::fmt` and finds nothing.
    let Some(m) = sibling("arioch") else { return };
    let r = analyse(&m);
    assert!(
        r.mixed_concern
            .iter()
            .any(|f| f.symbol == "ui::render_main"),
        "ui::render_main calls std::fs::metadata while building widgets"
    );
}
