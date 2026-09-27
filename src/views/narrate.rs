//! Narration slots, filled from the model at build time.
//!
//! metatron's README records why this mechanism exists, and it is worth
//! restating because the mistake is cheap to repeat: the first version had
//! its findings typed into the HTML, and when the charts updated for a new
//! project the paragraphs kept confidently describing the old one.
//!
//! A slot is declared `<p data-narr="layering"></p>`. Slots with something
//! to say are filled; slots with nothing to say are **removed**, not left
//! empty.

use crate::rules::{Kind, Status};
use crate::scorecard::Scorecard;

pub struct Narration {
    pub slots: Vec<(&'static str, String)>,
}

impl Narration {
    pub fn of(s: &Scorecard) -> Self {
        let mut slots: Vec<(&'static str, String)> = Vec::new();
        let cov = &s.classified.coverage;
        let e = s.enforcement();
        let c = s.counts();

        // coverage — first, for metatron's stated reason: a tool that
        // files half the code under "other" and then draws a confident
        // picture of it is worse than one that fails.
        slots.push((
            "coverage",
            if cov.classified == cov.total {
                format!(
                    "Every one of the {} classifiable symbols matched a pattern. \
                     The pictures below describe all of this crate, not part of it.",
                    cov.total
                )
            } else {
                format!(
                    "{} of {} symbols ({:.1}%) matched a pattern in metatron.toml. \
                     The remaining {} are drawn without a layer, and every view below \
                     is that much less than a description of this crate.",
                    cov.classified,
                    cov.total,
                    s.coverage(),
                    cov.total - cov.classified
                )
            },
        ));

        // ports — the seam, and whether it is testable
        let fakes = s
            .model
            .impls
            .iter()
            .filter(|b| b.trait_id.is_some())
            .filter(|b| b.is_test || b.file.ends_with("mem.rs"))
            .count();
        let real = s.model.impls.iter().filter(|b| b.trait_id.is_some()).count() - fakes;
        slots.push((
            "ports",
            if cov.ports == 0 {
                "No trait is defined under domain/ports/, so this crate has no port seam. \
                 Nothing calls through an interface it owns, and every rule with a port \
                 in its premise is unevaluable rather than passing."
                    .into()
            } else {
                format!(
                    "{} port{}, {real} real implementation{} and {fakes} fake{}. \
                     The upward arrows on the layers view are these: they correspond to \
                     no import, which is why a file-level tool cannot draw this seam.",
                    cov.ports,
                    plural(cov.ports),
                    plural(real),
                    plural(fakes),
                )
            },
        ));

        // enforcement — the axis the glossary adds and no other tool prints
        slots.push((
            "enforcement",
            if e.compiler == 0 {
                format!(
                    "Of {} rules, none is enforced by the compiler: this crate is \
                     crate-graph option A, a single crate where every boundary is \
                     convention. {} are checked here, {} delegated to clippy and {} \
                     advisory. Splitting domain into its own crate would move three \
                     of them into the build itself.",
                    c.rules, e.metatron, e.clippy, e.advisory
                )
            } else {
                format!(
                    "{} of {} rules are enforced by the compiler — an illegal import \
                     does not build. {} are checked here, and this tool's contribution \
                     shrinks as that first number grows.",
                    e.compiler, c.rules, e.metatron
                )
            },
        ));

        // unevaluable — the honesty slot
        let dark: Vec<&str> = s
            .report
            .findings
            .iter()
            .filter(|f| f.status == Status::Unevaluable)
            .map(|f| f.id)
            .collect();
        if !dark.is_empty() {
            slots.push((
                "unevaluable",
                format!(
                    "{} rule{} could not be judged at all, because {} premise{} does not \
                     exist here: {}. They are not passing.",
                    dark.len(),
                    plural(dark.len()),
                    if dark.len() == 1 { "its" } else { "their" },
                    plural(dark.len()),
                    dark.join(", ")
                ),
            ));
        }

        // layering
        let violated: Vec<&str> = s
            .report
            .findings
            .iter()
            .filter(|f| f.status == Status::Violated && f.kind == Kind::Decidable)
            .map(|f| f.id)
            .collect();
        if !violated.is_empty() {
            let n: usize = s
                .report
                .findings
                .iter()
                .filter(|f| f.status == Status::Violated && f.kind == Kind::Decidable)
                .map(|f| f.instances.len())
                .sum();
            slots.push((
                "layering",
                format!(
                    "{n} decidable violation{} across {} rule{}: {}. \
                     Each is a fact about the graph, not a judgement.",
                    plural(n),
                    violated.len(),
                    plural(violated.len()),
                    violated.join(", ")
                ),
            ));
        }

        // heuristics — reported prominently, gating nothing
        let heur: Vec<&str> = s
            .report
            .findings
            .iter()
            .filter(|f| f.status == Status::Violated && f.kind == Kind::Heuristic)
            .map(|f| f.id)
            .collect();
        if !heur.is_empty() {
            slots.push((
                "heuristics",
                format!(
                    "{} heuristic{} fired: {}. These have a real false-positive rate, \
                     so they are drawn and never gated — a ratchet that fails on a \
                     guess is switched off within a week.",
                    heur.len(),
                    plural(heur.len()),
                    heur.join(", ")
                ),
            ));
        }

        // cohesion
        let flags = s.cohesion_flags();
        if let Some(worst) = flags.first() {
            slots.push((
                "cohesion",
                format!(
                    "{} type{} over threshold. The widest is {} at {}:{} — {} fields, \
                     {} methods, LCOM4 {}, modularity {:.3}. {}",
                    flags.len(),
                    plural(flags.len()),
                    worst.name,
                    worst.file,
                    worst.line,
                    worst.field_count,
                    worst.method_count,
                    worst.lcom4,
                    worst.modularity,
                    match worst.verdict {
                        crate::cohesion::Verdict::Tangled => format!(
                            "There is no clean seam: the {} components share {} field{} \
                             across {} accesses, so nothing lifts out without dragging \
                             shared state with it.",
                            worst.components.len(),
                            worst.shared.len(),
                            plural(worst.shared.len()),
                            worst.cross_edges
                        ),
                        crate::cohesion::Verdict::Disconnected =>
                            "The pieces are already disjoint; the split is free.".into(),
                        _ => format!(
                            "The partition is strong enough to act on: {} components.",
                            worst.components.len()
                        ),
                    }
                ),
            ));
        }

        // the ratchet
        slots.push((
            "baseline",
            if !s.had_baseline {
                format!(
                    "No baseline is committed, so all {} violations read as new. \
                     `metatron baseline --update` accepts the current state and turns \
                     this into a ratchet.",
                    s.diff.new.len()
                )
            } else {
                format!(
                    "{} known, {} new, {} fixed since the committed baseline.",
                    s.diff.known.len(),
                    s.diff.new.len(),
                    s.diff.fixed.len()
                )
            },
        ));

        Narration { slots }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.slots.iter().find(|(k, _)| *k == key).map(|(_, v)| v.as_str())
    }

    pub fn paragraphs(&self) -> Vec<&str> {
        self.slots.iter().map(|(_, v)| v.as_str()).collect()
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

/// Fill `<... data-narr="key">...</...>` slots. A slot with nothing to say
/// is removed entirely, element and all.
pub fn fill(html: &str, s: &Scorecard) -> String {
    let n = Narration::of(s);
    let mut out = String::with_capacity(html.len());
    let mut rest = html;

    while let Some(at) = rest.find("data-narr=\"") {
        // Back up to the opening `<` of the element carrying the slot.
        let Some(open) = rest[..at].rfind('<') else {
            out.push_str(&rest[..at + 1]);
            rest = &rest[at + 1..];
            continue;
        };
        let tag: String = rest[open + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        let key_start = at + "data-narr=\"".len();
        let Some(key_len) = rest[key_start..].find('"') else { break };
        let key = &rest[key_start..key_start + key_len];

        let close = format!("</{tag}>");
        let Some(end) = rest[key_start..].find(&close).map(|i| key_start + i + close.len())
        else {
            break;
        };
        let Some(gt) = rest[key_start..end].find('>').map(|i| key_start + i + 1) else {
            break;
        };

        out.push_str(&rest[..open]);
        if let Some(text) = n.get(key) {
            // The marker is consumed, not kept: a `data-narr` surviving
            // into the output then always means a slot went unfilled,
            // which is a thing a test can assert.
            let open_tag = &rest[open..gt];
            let before = &open_tag[..at - open];
            let after = &open_tag[key_start + key_len + 1 - open..];
            out.push_str(before.trim_end());
            out.push_str(after);
            out.push_str(&escape(text));
            out.push_str(&close);
        }
        // else: the whole element is dropped.
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

fn escape(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}
