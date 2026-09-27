//! The violation ratchet. See `specs/05-scorecard-and-baseline.md`.
//!
//! Ported from metatron-nestjs rather than redesigned — that design is
//! sound and language-agnostic. Every detail is carried over deliberately:
//!
//! * The key is `sha1(rule|from|to)[..12]`, **not** a per-rule count. A
//!   count says "3 became 4"; only a fingerprint names the offender, and
//!   only a fingerprint catches a swap — one violation fixed and another
//!   introduced in the same change, which a count nets to zero and passes.
//! * `note` is hand-written and never overwritten. It is where the reason
//!   a violation is tolerated gets recorded, and it is what stops a
//!   baseline degrading into an unexamined list.
//! * Fixed violations are reported and never auto-removed. A scan that
//!   temporarily fails to parse a file would otherwise quietly retire a
//!   real debt, which returns later as a "new" violation with no history.

use crate::rules::{Finding, Kind, Report, Status};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const FILE: &str = "metatron.baseline.toml";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Entry {
    pub rule: String,
    pub from: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub to: String,
    /// Hand-written. Preserved across `--update` by fingerprint.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Progress {
    pub at: String,
    pub violations: usize,
    pub coverage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Baseline {
    pub version: u32,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub project: String,
    #[serde(default)]
    pub violations: BTreeMap<String, Entry>,
    #[serde(default)]
    pub progress: Vec<Progress>,
}

impl Default for Baseline {
    fn default() -> Self {
        Baseline {
            version: 1,
            generated_at: String::new(),
            project: String::new(),
            violations: BTreeMap::new(),
            progress: Vec::new(),
        }
    }
}

pub fn path_of(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

impl Baseline {
    pub fn load(dir: &Path) -> Result<Self> {
        let p = path_of(dir);
        if !p.exists() {
            return Ok(Self::default());
        }
        let text =
            std::fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", p.display()))
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        let p = path_of(dir);
        // Beside the config, not in `.metatron/`: output is generated and
        // gitignored, and a ratchet that is not committed cannot hold a
        // line.
        std::fs::write(&p, toml::to_string_pretty(self)?)
            .with_context(|| format!("writing {}", p.display()))
    }

    pub fn exists(dir: &Path) -> bool {
        path_of(dir).exists()
    }
}

/// `sha1(rule|from|to)` truncated to 12 hex digits.
pub fn fingerprint(rule: &str, from: &str, to: &str) -> String {
    let mut h = Sha1::new();
    h.update(format!("{rule}|{from}|{to}").as_bytes());
    format!("{:x}", h.finalize())[..12].to_string()
}

/// A rule may be excluded from the ratchet, and the reason is printed so
/// that nothing sits outside the gate unnoticed.
#[derive(Debug, Clone, Serialize)]
pub struct Excluded {
    pub rule: &'static str,
    pub why: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct Diff {
    pub new: Vec<(String, Entry)>,
    pub known: Vec<(String, Entry)>,
    pub fixed: Vec<(String, Entry)>,
    pub excluded: Vec<Excluded>,
}

impl Diff {
    /// Only a *gating* rule's new violations change the exit code.
    pub fn gating_new(&self, gating: &[&str]) -> usize {
        self.new
            .iter()
            .filter(|(_, e)| gating.contains(&e.rule.as_str()))
            .count()
    }
}

/// Every violation eligible for the ratchet, keyed by fingerprint.
///
/// Heuristic rules are excluded by construction, not by configuration:
/// spec 04 marks five rules `kind: heuristic` and they cannot enter here
/// under any flag. Non-gating decidable rules — `port-has-fake` warns
/// rather than fails — are excluded too, so the baseline holds exactly
/// what the exit code is allowed to depend on.
pub fn current(report: &Report) -> (BTreeMap<String, Entry>, Vec<Excluded>) {
    let mut out = BTreeMap::new();
    let mut excluded = Vec::new();
    for f in &report.findings {
        if f.kind == Kind::Heuristic {
            excluded.push(Excluded {
                rule: f.id,
                why: "heuristic — cannot gate",
            });
            continue;
        }
        if f.status == Status::Delegated {
            excluded.push(Excluded {
                rule: f.id,
                why: "delegated — guaranteed elsewhere",
            });
            continue;
        }
        if !f.gate {
            excluded.push(Excluded {
                rule: f.id,
                why: "advisory — warns, does not fail",
            });
            continue;
        }
        if f.status != Status::Violated {
            continue;
        }
        for i in &f.instances {
            let fp = fingerprint(f.id, &i.from, &i.to);
            out.insert(
                fp,
                Entry {
                    rule: f.id.to_string(),
                    from: i.from.clone(),
                    to: i.to.clone(),
                    note: String::new(),
                },
            );
        }
    }
    (out, excluded)
}

pub fn diff(report: &Report, base: &Baseline) -> Diff {
    let (cur, excluded) = current(report);
    let mut new = Vec::new();
    let mut known = Vec::new();
    for (fp, e) in &cur {
        match base.violations.get(fp) {
            // Carry the hand-written note onto the live violation.
            Some(b) => known.push((
                fp.clone(),
                Entry {
                    note: b.note.clone(),
                    ..e.clone()
                },
            )),
            None => new.push((fp.clone(), e.clone())),
        }
    }
    let fixed = base
        .violations
        .iter()
        .filter(|(fp, _)| !cur.contains_key(*fp))
        .map(|(fp, e)| (fp.clone(), e.clone()))
        .collect();
    Diff {
        new,
        known,
        fixed,
        excluded,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateOutcome {
    pub written: usize,
    pub notes_preserved: usize,
    /// Notes whose violation no longer exists. Reported separately, with
    /// an accurate count — metatron-nestjs shipped a bug here, reporting
    /// "1 note preserved" while preserving none.
    pub notes_dropped: Vec<(String, Entry)>,
}

/// Accept today's violations. Preserves every note whose fingerprint
/// survives, and reports the ones it had to drop.
pub fn update(
    report: &Report,
    base: &Baseline,
    project: &str,
    coverage: f64,
) -> (Baseline, UpdateOutcome) {
    let (cur, _) = current(report);
    let mut notes_preserved = 0;
    let mut violations = BTreeMap::new();
    for (fp, mut e) in cur {
        if let Some(old) = base.violations.get(&fp) {
            if !old.note.is_empty() {
                e.note = old.note.clone();
                notes_preserved += 1;
            }
        }
        violations.insert(fp, e);
    }
    let notes_dropped: Vec<(String, Entry)> = base
        .violations
        .iter()
        .filter(|(fp, e)| !e.note.is_empty() && !violations.contains_key(*fp))
        .map(|(fp, e)| (fp.clone(), e.clone()))
        .collect();

    let today = today();
    let mut progress = base.progress.clone();
    let n = violations.len();
    // One entry per acceptance, and one per day at most: re-running
    // `--update` should not manufacture a trend.
    match progress.last_mut() {
        Some(p) if p.at == today => {
            p.violations = n;
            p.coverage = coverage;
        }
        _ => progress.push(Progress {
            at: today.clone(),
            violations: n,
            coverage,
        }),
    }

    let out = UpdateOutcome {
        written: n,
        notes_preserved,
        notes_dropped,
    };
    (
        Baseline {
            version: 1,
            generated_at: now_iso(),
            project: project.to_string(),
            violations,
            progress,
        },
        out,
    )
}

// ------------------------------------------------------------------ dates

fn epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Civil date from a day count (Howard Hinnant's algorithm). Cheaper than
/// a date crate for the one thing this file needs.
fn ymd(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn today() -> String {
    let (y, m, d) = ymd((epoch_secs() / 86_400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn now_iso() -> String {
    let s = epoch_secs();
    let (y, m, d) = ymd((s / 86_400) as i64);
    let t = s % 86_400;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        t / 3600,
        (t % 3600) / 60,
        t % 60
    )
}

/// Rules whose new violations are allowed to change the exit code.
pub fn gating_rules(report: &Report) -> Vec<&'static str> {
    report
        .findings
        .iter()
        .filter(|f: &&Finding| f.gate)
        .map(|f| f.id)
        .collect()
}
