//! Acceptance tests for `specs/11-workbench.md`: the wiring model.
//!
//! The `wiring` fixture holds one of each shape the workbench has to tell
//! apart — a port held in a field, a port taken through a generic, a type
//! nobody declared, a type from another crate, a trait the crate does not
//! own, a fake that must not count as a binding, an entry nothing calls,
//! locals whose `let` declares their type, and one whose `let` does not.

use metatron::classify::{self, Config};
use metatron::model::{EdgeKind, EdgeTarget, Model};
use metatron::views::workbench;
use metatron::wiring::{Brick, Wiring};
use std::path::PathBuf;

fn dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn wiring(name: &str) -> (Model, Wiring) {
    let p = dir(name);
    let model = metatron::scan(&p).expect("scan");
    let config = Config::load(&p).expect("config");
    let classified = classify::classify(&model, &config);
    let w = workbench::wiring(&model, &config, &classified);
    (model, w)
}

fn brick<'a>(w: &'a Wiring, id: &str) -> &'a Brick {
    w.bricks.iter().find(|b| b.id == id).unwrap_or_else(|| {
        panic!(
            "no brick {id}; have {:?}",
            w.bricks.iter().map(|b| &b.id).collect::<Vec<_>>()
        )
    })
}

fn stud<'a>(b: &'a Brick, name: &str) -> &'a metatron::wiring::Stud {
    b.studs
        .iter()
        .find(|s| s.name == name)
        .unwrap_or_else(|| panic!("{} has no stud {name}", b.id))
}

fn socket<'a>(b: &'a Brick, prop: &str) -> &'a metatron::wiring::Socket {
    b.sockets
        .iter()
        .find(|s| s.prop.split(", ").any(|p| p == prop))
        .unwrap_or_else(|| panic!("{} has no socket {prop}: {:?}", b.id, b.sockets))
}

// ------------------------------------------------------------- the scanner

#[test]
fn a_field_edge_names_its_field_and_a_param_edge_its_parameter() {
    let (m, _) = wiring("wiring");
    let via = |from: &str, kind: EdgeKind, to: &str| {
        m.edges
            .iter()
            .find(|e| {
                e.from == from
                    && e.kind == kind
                    && matches!(&e.to, EdgeTarget::Local { id } if id == to)
            })
            .and_then(|e| e.via.clone())
    };
    assert_eq!(
        via("app::App", EdgeKind::Field, "app::Ledger").as_deref(),
        Some("ledger")
    );
    assert_eq!(
        via("app::App", EdgeKind::Field, "ports::Clock").as_deref(),
        Some("clock")
    );
    // `S: Store` is named for the parameter typed `&S`, not left anonymous.
    assert_eq!(
        via("app::cmd_tick", EdgeKind::Bound, "ports::Store").as_deref(),
        Some("store")
    );
}

#[test]
fn receiver_calls_are_recorded_raw_and_make_no_edge() {
    let (m, _) = wiring("wiring");
    let tick = m.symbol("app::App::tick").unwrap();
    let raw: Vec<(&str, &str)> = tick
        .recv_calls
        .iter()
        .map(|c| (c.recv.as_str(), c.method.as_str()))
        .collect();
    assert!(raw.contains(&("self.clock", "now")), "{raw:?}");
    assert!(raw.contains(&("self.ledger", "record")), "{raw:?}");
    // Not an edge: no rule reads them, so no rule's answer moves.
    assert!(!m.edges.iter().any(|e| e.kind == EdgeKind::Call
        && e.from == "app::App::tick"
        && matches!(&e.to, EdgeTarget::Local { id } if id == "app::Ledger::record")));
}

#[test]
fn an_impl_block_lists_its_fns_and_a_method_its_receiver() {
    let (m, _) = wiring("wiring");
    let display = m
        .impls
        .iter()
        .find(|b| b.trait_path.ends_with("Display"))
        .unwrap();
    assert_eq!(display.methods, ["fmt"]);
    let tick = m.symbol("app::App::tick").unwrap();
    assert_eq!(
        tick.sig.as_ref().unwrap().receiver.as_deref(),
        Some("&mut self")
    );
}

// -------------------------------------------------------------- bricks

#[test]
fn types_traits_and_each_modules_functions_are_bricks_and_tests_are_not() {
    let (_, w) = wiring("wiring");
    let ids: Vec<&str> = w.bricks.iter().map(|b| b.id.as_str()).collect();
    for want in [
        "app::App",
        "app::Ledger",
        "app::*",
        "infra::Wall",
        "infra::Sqlite",
        "ports::Clock",
        "ports::Store",
        "(crate)::*",
    ] {
        assert!(ids.contains(&want), "missing {want}: {ids:?}");
    }
    assert!(
        !ids.iter().any(|i| i.contains("Frozen")),
        "a test fake is a brick: {ids:?}"
    );
    assert_eq!(brick(&w, "ports::Clock").shape, "trait");
    assert_eq!(brick(&w, "app::*").shape, "functions");
    assert_eq!(brick(&w, "(crate)::*").name, "main.rs");
}

#[test]
fn bricks_are_stable_across_scans() {
    let ids = |w: &Wiring| w.bricks.iter().map(|b| b.id.clone()).collect::<Vec<_>>();
    let (_, a) = wiring("wiring");
    let (_, b) = wiring("wiring");
    assert_eq!(ids(&a), ids(&b));
    assert_eq!(a.calls, b.calls);
}

// ------------------------------------------------------------- sockets

#[test]
fn a_port_held_in_a_field_is_bound_to_its_one_impl_outside_tests() {
    let (_, w) = wiring("wiring");
    let s = socket(brick(&w, "app::App"), "clock");
    assert_eq!(s.status, "port");
    assert_eq!(s.to.as_deref(), Some("ports::Clock"));
    // `Frozen` implements `Clock` too, under `#[cfg(test)]`: not a binding.
    assert_eq!(s.bound_to.as_deref(), Some("infra::Wall"));
}

#[test]
fn a_port_taken_through_a_generic_is_a_socket_of_the_function() {
    let (_, w) = wiring("wiring");
    let s = socket(brick(&w, "app::*"), "store");
    assert_eq!(s.status, "port");
    assert_eq!(s.via, "param");
    assert_eq!(s.to.as_deref(), Some("ports::Store"));
}

#[test]
fn a_type_from_another_crate_is_external_and_std_is_not_a_socket() {
    let (_, w) = wiring("wiring");
    let s = socket(brick(&w, "infra::Sqlite"), "conn");
    assert_eq!(s.status, "external");
    assert_eq!(s.external.as_deref(), Some("rusqlite::Connection"));
    // `entries: Vec<u64>` is data.
    assert!(brick(&w, "app::Ledger").sockets.is_empty());
}

#[test]
fn a_type_nobody_declared_is_an_unresolved_socket_with_a_diagnostic() {
    let (_, w) = wiring("wiring");
    let s = socket(brick(&w, "app::App"), "ghost");
    assert_eq!(s.status, "unresolved");
    assert!(s.why.as_deref().unwrap_or("").contains("Phantom"), "{s:?}");
    assert!(w
        .diagnostics
        .iter()
        .any(|d| d.kind == "socket-unresolved" && d.detail.contains("ghost")));
    assert_eq!(w.meta.unresolved, 1);
    assert_eq!(w.meta.resolved + w.meta.unresolved, w.meta.sockets);
}

#[test]
fn a_generic_parameter_is_not_an_unresolved_socket() {
    // `S` raises "could not be placed" in the scan; it is the port's
    // generic, and the workbench must not count it as a gap.
    let (_, w) = wiring("wiring");
    assert!(!brick(&w, "app::*")
        .sockets
        .iter()
        .any(|s| s.status == "unresolved"));
}

// --------------------------------------------------------------- calls

#[test]
fn a_call_on_a_field_follows_the_fields_declared_type() {
    let (_, w) = wiring("wiring");
    let to = |m: &str| {
        w.calls
            .iter()
            .find(|c| c.from == "app::App" && c.to_method.as_deref() == Some(m))
            .unwrap_or_else(|| panic!("no call App -> .{m}: {:?}", w.calls))
    };
    let now = to("now");
    assert_eq!((now.to.as_str(), now.via), ("ports::Clock", "field"));
    assert_eq!(now.bound_to.as_deref(), Some("infra::Wall"));
    assert_eq!(to("record").to, "app::Ledger");
    // `self.helper()` is wiring inside one brick, not between two.
    assert!(!w
        .calls
        .iter()
        .any(|c| c.to_method.as_deref() == Some("helper")));
}

fn calls_from<'a>(w: &'a Wiring, brick: &str, method: &str) -> Vec<(&'a str, &'a str, &'a str)> {
    w.calls
        .iter()
        .filter(|c| c.from == brick && c.from_method.as_deref() == Some(method))
        .map(|c| (c.to.as_str(), c.to_method.as_deref().unwrap_or(""), c.via))
        .collect()
}

#[test]
fn a_call_on_a_parameter_follows_its_bound() {
    let (_, w) = wiring("wiring");
    let from_tick = calls_from(&w, "app::*", "cmd_tick");
    assert!(
        from_tick.contains(&("ports::Store", "load", "param")),
        "{from_tick:?}"
    );
    assert!(
        from_tick.contains(&("ports::Store", "save", "param")),
        "{from_tick:?}"
    );
}

#[test]
fn a_local_is_followed_when_its_let_declares_its_type_and_not_otherwise() {
    let (_, w) = wiring("wiring");
    // `let built = Ledger { .. }`: a struct literal names its type.
    assert!(calls_from(&w, "app::*", "cmd_tick").contains(&("app::Ledger", "total", "local")));
    // `let mut a = App::new(..)`: `new` is declared `-> Self`.
    assert!(calls_from(&w, "(crate)::*", "main").contains(&("app::App", "tick", "local")));
    // `Sqlite::open().vacuum()`: the receiver is what `open` returns.
    assert!(calls_from(&w, "(crate)::*", "main").contains(&("infra::Sqlite", "vacuum", "chain")));
    // `let found = app.ledger()`: a method's result would have to be inferred.
    let idle = calls_from(&w, "app::*", "cmd_idle");
    assert!(!idle.iter().any(|c| c.1 == "clear"), "{idle:?}");
    assert!(w.meta.recv_calls.unread >= 1);
    assert_eq!(w.meta.recv_calls.ambiguous, 0);
}

#[test]
fn an_associated_fn_called_by_path_is_a_static_call() {
    let (_, w) = wiring("wiring");
    let open = w
        .calls
        .iter()
        .find(|c| c.to == "infra::Sqlite" && c.to_method.as_deref() == Some("open"))
        .expect("main -> Sqlite::open");
    assert!(open.is_static);
    assert_eq!(open.from, "(crate)::*");
}

// --------------------------------------------------------------- studs

#[test]
fn every_stud_says_what_grips_it() {
    let (_, w) = wiring("wiring");
    let grip = |b: &str, s: &str| stud(brick(&w, b), s).grip.unwrap_or("-");
    assert_eq!(grip("ports::Clock", "now"), "brick");
    assert_eq!(grip("ports::Store", "load"), "brick");
    // Nobody calls `Wall::now` by name; App calls `Clock::now`.
    assert_eq!(grip("infra::Wall", "now"), "port");
    assert_eq!(grip("infra::Sqlite", "save"), "port");
    // `Display::fmt` is called by std.
    assert_eq!(grip("infra::Sqlite", "fmt"), "framework");
    assert_eq!(
        stud(brick(&w, "infra::Sqlite"), "fmt").framework.as_deref(),
        Some("impl std::fmt::Display")
    );
    // An entry nothing in the crate calls.
    assert_eq!(grip("app::*", "cmd_idle"), "route");
    assert_eq!(grip("app::*", "cmd_tick"), "brick");
    assert!(stud(brick(&w, "app::*"), "cmd_tick").route.is_some());
    assert_eq!(grip("app::App", "idle"), "brick");
    assert_eq!(grip("app::App", "tick"), "brick");
    assert_eq!(grip("app::Ledger", "total"), "brick");
    // Only ever called on a local nobody declared the type of.
    assert_eq!(grip("app::Ledger", "clear"), "unseen");
    assert_eq!(grip("app::App", "unused"), "unseen");
}

#[test]
fn a_trait_impls_fns_are_studs_though_written_without_pub() {
    let (_, w) = wiring("wiring");
    let wall = brick(&w, "infra::Wall");
    assert_eq!(stud(wall, "now").of_trait.as_deref(), Some("Clock"));
    assert!(wall.internals.is_empty());
    // A private inherent fn is an internal, not a stud.
    let app = brick(&w, "app::App");
    assert!(app.internals.iter().any(|s| s.name == "helper"));
    assert!(!app.studs.iter().any(|s| s.name == "helper"));
    assert_eq!(
        stud(app, "new").sig,
        "fn new(clock: Box<dyn Clock>) -> Self"
    );
    assert!(stud(app, "new").is_static);
}

#[test]
fn every_impl_of_a_port_is_listed_but_a_fakes_is_not() {
    let (_, w) = wiring("wiring");
    let mut impls: Vec<(&str, &str)> = w
        .impls
        .iter()
        .map(|i| (i.of_trait.as_str(), i.ty.as_str()))
        .collect();
    impls.sort();
    assert_eq!(
        impls,
        [
            ("ports::Clock", "infra::Wall"),
            ("ports::Store", "infra::Sqlite")
        ]
    );
}

// ------------------------------------------------------------ fixtures

#[test]
fn the_ports_fixture_draws_its_seam() {
    let (_, w) = wiring("ports");
    // Two concretes fill `ActivityStore`: the binding is main.rs's choice,
    // and the workbench does not pick one.
    let cli = brick(&w, "application::cli::*");
    let store = socket(cli, "store");
    assert_eq!(store.status, "port");
    assert_eq!(store.bound_to, None);
    assert_eq!(
        socket(cli, "clock").bound_to.as_deref(),
        Some("infra::mem::FixedClock")
    );
    assert_eq!(
        stud(
            brick(&w, "infra::sqlite::SqliteActivityStore"),
            "open_session"
        )
        .grip,
        Some("port")
    );
    assert_eq!(w.meta.unresolved, 0);
}

#[test]
fn a_brick_has_a_tier_and_a_pattern_from_the_classification() {
    let p = dir("ports");
    let model = metatron::scan(&p).unwrap();
    let config = Config::load(&p).unwrap();
    let classified = classify::classify(&model, &config);
    let w = workbench::wiring(&model, &config, &classified);
    let layer = |id: &str| config.layers[brick(&w, id).tier].id.as_str();
    assert_eq!(
        layer("domain::ports::activity_store::ActivityStore"),
        "domain"
    );
    assert_eq!(
        layer("infra::sqlite::SqliteActivityStore"),
        "infrastructure"
    );
    assert_eq!(
        brick(&w, "domain::use_cases::activity::*").pattern,
        "use-case"
    );
}
