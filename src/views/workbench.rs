//! The workbench payload. See `specs/11-workbench.md`.
//!
//! Every other lens is a file with its data baked in. This one is served
//! (`crate::serve`), because reading code needs the source and a workbench
//! should follow the code as it changes — so it is built from the scan, the
//! config and the classification alone, without the git history and rules
//! a [`crate::Scorecard`] carries, and is cheap enough to rebuild on save.

use super::{tiers_of, traffic, Tier};
use crate::classify::{Classified, Config};
use crate::model::Model;
use crate::wiring::{self, Brick, Call, ImplLink, Wire, Wiring, WiringDiagnostic, WiringMeta};
use serde::Serialize;
use std::collections::BTreeMap;

/// The wiring, with the traffic view's entries as the studs gripped from
/// outside the code.
pub fn wiring(model: &Model, config: &Config, classified: &Classified) -> Wiring {
    let entries: BTreeMap<String, String> = model
        .symbols
        .iter()
        .filter_map(|s| traffic::entry_verb(s, classified).map(|v| (s.id.clone(), v.into())))
        .collect();
    wiring::build(model, config, classified, &entries)
}

/// An entry point, in the slots the page reads for an HTTP endpoint.
#[derive(Serialize, Clone, Debug)]
pub struct Endpoint {
    pub id: String,
    /// `cli` or `tui`.
    pub verb: String,
    pub route: String,
    pub file: String,
    pub cls: String,
    pub handler: String,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct Payload {
    pub version: u64,
    pub project: String,
    pub generated_at: String,
    pub tiers: Vec<Tier>,
    pub bricks: Vec<Brick>,
    pub wires: Vec<Wire>,
    pub calls: Vec<Call>,
    pub impls: Vec<ImplLink>,
    pub wiring_meta: WiringMeta,
    pub endpoints: Vec<Endpoint>,
    pub diagnostics: Vec<WiringDiagnostic>,
    /// Whether the server can show a change (metatron-nestjs spec 11). Not
    /// yet: that needs `metatron diff` (spec 10), so the page hides the form
    /// rather than offering a box that can only fail.
    pub changes: bool,
    /// The last re-scan's error. The model beside it is the last good one.
    pub error: Option<String>,
    pub watching: bool,
}

pub fn payload(model: &Model, config: &Config, classified: &Classified, now_secs: u64) -> Payload {
    let w = wiring(model, config, classified);
    let endpoints = model
        .symbols
        .iter()
        .filter_map(|s| {
            let verb = traffic::entry_verb(s, classified)?;
            Some(Endpoint {
                id: s.id.clone(),
                verb: verb.into(),
                route: s.name.clone(),
                file: s.file.clone(),
                cls: s.parent.clone().unwrap_or_else(|| s.module.clone()),
                handler: s.name.clone(),
            })
        })
        .collect();
    Payload {
        version: 0,
        project: model.project.clone(),
        generated_at: crate::baseline::today_from(now_secs),
        tiers: tiers_of(config),
        bricks: w.bricks,
        wires: w.wires,
        calls: w.calls,
        impls: w.impls,
        wiring_meta: w.meta,
        endpoints,
        diagnostics: w.diagnostics,
        changes: false,
        error: None,
        watching: false,
    }
}
