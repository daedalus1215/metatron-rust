//! `cohesion` — the view the new model makes possible, and the one with
//! the most to say about a crate that has no architecture yet.
//!
//! For each type over threshold: a matrix, methods down one axis and
//! fields across the other, cells marked where a method touches a field,
//! rows and columns ordered so components fall into visible blocks.
//! Off-diagonal marks are the cross-component accesses — the cost of the
//! split, drawn.
//!
//! This is the standard way to present LCOM and it reads instantly: a
//! cohesive type is one block; a tangled one is blocks with a smear of
//! shared access running through the columns everything touches.

use crate::scorecard::Scorecard;
use serde::Serialize;

#[derive(Serialize)]
pub struct Matrix {
    pub symbol: String,
    pub name: String,
    pub file: String,
    pub line: u32,
    pub verdict: String,
    pub lcom4: usize,
    pub modularity: f64,
    /// Column labels, ordered by component.
    pub fields: Vec<String>,
    /// Row labels, ordered by component.
    pub methods: Vec<String>,
    /// `cells[row]` is the list of column indices that row touches.
    pub cells: Vec<Vec<usize>>,
    /// Where each component starts, for drawing the block boundaries.
    pub blocks: Vec<Block>,
    /// Columns reached from more than one block: the shared state a split
    /// has to deal with first.
    pub shared: Vec<usize>,
    pub cross_edges: usize,
    pub unused: Vec<String>,
    pub write_only: Vec<String>,
}

#[derive(Serialize)]
pub struct Block {
    pub name: String,
    pub row: usize,
    pub rows: usize,
    pub col: usize,
    pub cols: usize,
    pub loc: u32,
}

#[derive(Serialize)]
pub struct Cohesion {
    #[serde(rename = "generatedAt")]
    pub generated_at: String,
    pub project: String,
    pub types: Vec<Matrix>,
    pub mixed_concern: Vec<MixedConcern>,
}

#[derive(Serialize)]
pub struct MixedConcern {
    pub symbol: String,
    pub file: String,
    pub line: u32,
    pub concerns: Vec<String>,
}

pub fn build(s: &Scorecard) -> Cohesion {
    let types = s
        .cohesion_flags()
        .into_iter()
        .map(|t| {
            // Order rows and columns by component, so the blocks are
            // contiguous and the off-diagonal marks are the finding.
            let mut fields: Vec<String> = Vec::new();
            let mut methods: Vec<String> = Vec::new();
            let mut blocks = Vec::new();
            for c in &t.components {
                let (row, col) = (methods.len(), fields.len());
                methods.extend(c.methods.iter().cloned());
                fields.extend(c.fields.iter().cloned());
                blocks.push(Block {
                    name: c
                        .name
                        .clone()
                        .unwrap_or_else(|| format!("group {}", blocks.len() + 1)),
                    row,
                    rows: c.methods.len(),
                    col,
                    cols: c.fields.len(),
                    loc: c.loc,
                });
            }
            // Fields nothing touches still belong on the axis: dead state
            // is part of the picture.
            for f in t.unused_fields.iter().chain(t.write_only_fields.iter()) {
                if !fields.contains(f) {
                    fields.push(f.clone());
                }
            }

            let col_of = |f: &str| fields.iter().position(|x| x == f);
            let cells: Vec<Vec<usize>> = methods
                .iter()
                .map(|m| {
                    s.model
                        .symbols
                        .iter()
                        .find(|x| x.parent.as_deref() == Some(t.symbol.as_str()) && &x.name == m)
                        .map(|x| x.self_fields.iter().filter_map(|f| col_of(f)).collect())
                        .unwrap_or_default()
                })
                .collect();

            Matrix {
                symbol: t.symbol.clone(),
                name: t.name.clone(),
                file: t.file.clone(),
                line: t.line,
                verdict: format!("{:?}", t.verdict).to_lowercase(),
                lcom4: t.lcom4,
                modularity: (t.modularity * 1000.0).round() / 1000.0,
                shared: t.shared.iter().filter_map(|f| col_of(f)).collect(),
                fields,
                methods,
                cells,
                blocks,
                cross_edges: t.cross_edges,
                unused: t.unused_fields.clone(),
                write_only: t.write_only_fields.clone(),
            }
        })
        .collect();

    Cohesion {
        generated_at: crate::baseline::now_iso(),
        project: s.model.project.clone(),
        types,
        mixed_concern: s
            .cohesion
            .mixed_concern
            .iter()
            .map(|f| MixedConcern {
                symbol: f.symbol.clone(),
                file: f.file.clone(),
                line: f.line,
                concerns: f.concerns.keys().cloned().collect(),
            })
            .collect(),
    }
}
