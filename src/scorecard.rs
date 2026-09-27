//! The scorecard. See `specs/05-scorecard-and-baseline.md`.
//!
//! There is deliberately **no single conformance percentage**. It is the
//! most requestable and least defensible output this tool could produce:
//! it averages rules of wildly different weight, it moves for reasons
//! nobody can reconstruct, and it has to decide what to do with
//! unevaluable rules — counting them as failures punishes a project for
//! not having ports yet, counting them as passes is a lie, and excluding
//! them inflates the score of a codebase with almost no architecture to
//! grade.
//!
//! Three counts and an exposure line instead.

use crate::baseline::{self, Baseline, Diff};
use crate::classify::{self, Classified, Config};
use crate::cohesion::{self, CohesionReport, Verdict};
use crate::model::Model;
use crate::rules::{self, Kind, Report, Status, Tier};
use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Counts {
    pub rules: usize,
    pub upheld: usize,
    pub violated: usize,
    pub unevaluable: usize,
    pub passing: usize,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Enforcement {
    pub compiler: usize,
    pub metatron: usize,
    pub clippy: usize,
    pub advisory: usize,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Delta {
    pub violations: i64,
    pub coverage: f64,
    pub since: Option<&'static str>,
}

pub struct Scorecard {
    pub dir: PathBuf,
    pub model: Model,
    pub config: Config,
    pub classified: Classified,
    pub report: Report,
    pub cohesion: CohesionReport,
    pub baseline: Baseline,
    pub diff: Diff,
    pub had_baseline: bool,
}

pub fn build(dir: &Path) -> Result<Scorecard> {
    let model = crate::scan(dir)?;
    let config = Config::load(dir)?;
    let classified = classify::classify(&model, &config);
    let report = rules::check(&model, &config, &classified);
    let cohesion = cohesion::analyse(&model);
    let had_baseline = Baseline::exists(dir);
    let baseline = Baseline::load(dir)?;
    let diff = baseline::diff(&report, &baseline);
    Ok(Scorecard {
        dir: dir.to_path_buf(),
        model,
        config,
        classified,
        report,
        cohesion,
        baseline,
        diff,
        had_baseline,
    })
}

impl Scorecard {
    pub fn counts(&self) -> Counts {
        let f = &self.report.findings;
        Counts {
            rules: f.len(),
            upheld: f.iter().filter(|x| x.status == Status::Upheld).count(),
            violated: f.iter().filter(|x| x.status == Status::Violated).count(),
            unevaluable: f.iter().filter(|x| x.status == Status::Unevaluable).count(),
            passing: f
                .iter()
                .filter(|x| matches!(x.status, Status::Pass | Status::Delegated))
                .count(),
        }
    }

    /// The line the glossary earns and no other tool prints. It changes on
    /// its own as the project matures: do the Option B workspace split and
    /// three rules migrate from `metatron` to `compiler`.
    pub fn enforcement(&self) -> Enforcement {
        let f = &self.report.findings;
        Enforcement {
            compiler: f.iter().filter(|x| x.tier == Tier::Compiler).count(),
            metatron: f
                .iter()
                .filter(|x| x.gate && x.tier != Tier::Compiler && x.status != Status::Delegated)
                .count(),
            clippy: f.iter().filter(|x| x.tier == Tier::Clippy).count(),
            advisory: f
                .iter()
                .filter(|x| !x.gate && x.tier != Tier::Clippy && x.tier != Tier::Compiler)
                .count(),
        }
    }

    pub fn coverage(&self) -> f64 {
        self.classified.coverage.ratio() * 100.0
    }

    pub fn gating_rules(&self) -> Vec<&'static str> {
        baseline::gating_rules(&self.report)
    }

    /// New violations from rules that are allowed to change the exit code.
    pub fn new_gating(&self) -> Vec<&(String, baseline::Entry)> {
        let g = self.gating_rules();
        self.diff
            .new
            .iter()
            .filter(|(_, e)| g.contains(&e.rule.as_str()))
            .collect()
    }

    /// Movement since the last accepted baseline. Data only — nothing
    /// gates on the trend, because a project is allowed to have a bad week.
    pub fn delta(&self) -> Option<Delta> {
        let last = self.baseline.progress.last()?;
        Some(Delta {
            violations: (self.diff.new.len() + self.diff.known.len()) as i64
                - last.violations as i64,
            coverage: self.coverage() - last.coverage,
            since: None,
        })
    }

    pub fn cohesion_flags(&self) -> Vec<&crate::cohesion::TypeCohesion> {
        self.cohesion
            .types
            .iter()
            .filter(|t| matches!(t.verdict, Verdict::Tangled | Verdict::Splittable | Verdict::Disconnected))
            .collect()
    }

    pub fn exit_code(&self, allow_new: usize) -> i32 {
        if self.new_gating().len() > allow_new {
            1
        } else {
            0
        }
    }

    // ------------------------------------------------------------- test API

    /// Panics with the report if any new gating violation appeared.
    /// `cargo test` output is the only thing anyone will read, so the
    /// panic message *is* the report.
    #[track_caller]
    pub fn assert_no_new_violations(&self) {
        self.assert_at_most_new(0);
    }

    #[track_caller]
    pub fn assert_at_most_new(&self, allow: usize) {
        let new = self.new_gating();
        if new.len() <= allow {
            return;
        }
        let mut msg = format!(
            "metatron: {} new architecture violation(s) in `{}`",
            new.len(),
            self.model.project
        );
        if allow > 0 {
            msg.push_str(&format!(" (allowing {allow})"));
        }
        msg.push_str("\n\n");
        for (fp, e) in &new {
            let where_ = self
                .report
                .findings
                .iter()
                .find(|f| f.id == e.rule)
                .and_then(|f| {
                    f.instances
                        .iter()
                        .find(|i| i.from == e.from && i.to == e.to)
                })
                .map(|i| format!("{}:{}  {}", i.file, i.line, i.detail))
                .unwrap_or_else(|| format!("{} -> {}", e.from, e.to));
            msg.push_str(&format!("  {:<22} {where_}\n", e.rule));
            msg.push_str(&format!("  {:<22} fingerprint {fp}\n", ""));
        }
        msg.push_str(&format!(
            "\n  {} known violation(s) in {}.\n",
            self.diff.known.len(),
            baseline::FILE
        ));
        if !self.had_baseline {
            msg.push_str(
                "  No baseline exists yet. Run `metatron baseline` to accept \
                 the current state.\n",
            );
        }
        panic!("{msg}");
    }

    /// Assert the tool recognised enough of the crate to be worth
    /// believing. A conformance run over code it classified nothing in is
    /// lying by omission, and a test can say so.
    #[track_caller]
    pub fn assert_coverage_at_least(&self, pct: f64) {
        let got = self.coverage();
        assert!(
            got >= pct,
            "metatron: classified {:.1}% of symbols, expected at least {pct:.1}%.\n\
             {} of {} symbols matched no pattern in metatron.toml.",
            got,
            self.classified.unmatched.len(),
            self.classified.coverage.total
        );
    }

    /// Assert no rule silently reports a pass it did not earn.
    #[track_caller]
    pub fn assert_no_unevaluable(&self) {
        let dark: Vec<&str> = self
            .report
            .findings
            .iter()
            .filter(|f| f.kind == Kind::Decidable && f.status == Status::Unevaluable)
            .map(|f| f.id)
            .collect();
        assert!(
            dark.is_empty(),
            "metatron: {} decidable rule(s) could not be evaluated: {}\n\
             Their premises do not exist in this crate; they are not passing.",
            dark.len(),
            dark.join(", ")
        );
    }
}
