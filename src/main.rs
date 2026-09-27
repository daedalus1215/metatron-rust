use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "metatron",
    version,
    about = "Rust architecture model and conformance checker"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
    /// Crate directory (the one holding Cargo.toml). Defaults to the cwd.
    path: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Parse to .metatron/model.json. No analysis.
    Scan {
        path: Option<PathBuf>,
        /// Print the model to stdout instead of writing it.
        #[arg(long)]
        stdout: bool,
    },
    /// Classify every symbol against `metatron.toml` and report coverage.
    Classify {
        path: Option<PathBuf>,
        /// List every unmatched symbol rather than grouping them.
        #[arg(long)]
        verbose: bool,
        /// Emit the classification as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Evaluate the rules against the baseline. Exit 1 on a new violation.
    Check {
        path: Option<PathBuf>,
        /// Show passing and unevaluable rules too.
        #[arg(long)]
        all: bool,
        /// Emit the report as JSON.
        #[arg(long)]
        json: bool,
        /// Gate on these rules only; the rest become advisory.
        #[arg(long = "rule")]
        rules: Vec<String>,
        /// Tolerate up to n new violations. A number in CI config, so
        /// lowering it is a visible, reviewable act.
        #[arg(long, default_value_t = 0)]
        allow_new: usize,
        /// Suppress the fixed-since-baseline section.
        #[arg(long)]
        no_fixed: bool,
    },
    /// Render the views to .metatron/views/.
    Views {
        path: Option<PathBuf>,
        /// Render one view instead of all of them.
        name: Option<String>,
        /// Write here instead of .metatron/views/.
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Record today's violations as accepted.
    Baseline {
        path: Option<PathBuf>,
        /// Write the file. Without it, print what would change.
        #[arg(long)]
        update: bool,
    },
    /// LCOM4 and decomposition proposals. Advisory: never gates.
    Cohesion {
        path: Option<PathBuf>,
        /// Include types reported as cohesive.
        #[arg(long)]
        all: bool,
        /// Emit the report as JSON.
        #[arg(long)]
        json: bool,
    },
}

/// Exit codes, named so the dispatch below reads as the policy it is:
///
/// * `0` — the command ran and the answer is in the output.
/// * `1` — the command ran and the answer is "no": a regression, or a gate that
///   did not hold. CI fails; nothing is wrong with the tool.
/// * `2` — the command did not produce an answer: a missing directory, an
///   unparseable manifest, a template that would not render. Nothing was
///   checked, so `1` would be a lie about work never done.
const REGRESSION: u8 = 1;
const NO_ANSWER: u8 = 2;

fn main() -> std::process::ExitCode {
    match run() {
        Ok(code) => std::process::ExitCode::from(code),
        Err(e) => {
            eprintln!("metatron: {e:#}");
            std::process::ExitCode::from(NO_ANSWER)
        }
    }
}

fn run() -> Result<u8> {
    let cli = Cli::parse();
    let (path, to_stdout) = match cli.command {
        Some(Cmd::Scan { path, stdout }) => (path.or(cli.path), stdout),
        Some(Cmd::Classify {
            path,
            verbose,
            json,
        }) => {
            let dir = path.or(cli.path).unwrap_or_else(|| PathBuf::from("."));
            return classify(&dir, verbose, json);
        }
        Some(Cmd::Check {
            path,
            all,
            json,
            rules,
            allow_new,
            no_fixed,
        }) => {
            let dir = path.or(cli.path).unwrap_or_else(|| PathBuf::from("."));
            return check(&dir, all, json, &rules, allow_new, no_fixed);
        }
        Some(Cmd::Views { path, name, out }) => {
            let dir = path.or(cli.path).unwrap_or_else(|| PathBuf::from("."));
            return views(&dir, name.as_deref(), out);
        }
        Some(Cmd::Baseline { path, update }) => {
            let dir = path.or(cli.path).unwrap_or_else(|| PathBuf::from("."));
            return baseline_cmd(&dir, update);
        }
        Some(Cmd::Cohesion { path, all, json }) => {
            let dir = path.or(cli.path).unwrap_or_else(|| PathBuf::from("."));
            return cohesion(&dir, all, json);
        }
        None => (cli.path, false),
    };
    let dir = path.unwrap_or_else(|| PathBuf::from("."));

    let model = metatron::scan(&dir)?;
    let json = serde_json::to_string_pretty(&model)?;

    if to_stdout {
        println!("{json}");
        return Ok(0);
    }

    let out = dir.join(".metatron");
    std::fs::create_dir_all(&out)?;
    std::fs::write(out.join("model.json"), &json)?;

    let s = &model.stats;
    println!("metatron scan · {}", model.project);
    println!();
    println!(
        "  {} files · {} modules · {} loc",
        s.files, s.modules, s.loc
    );
    println!(
        "  {} symbols ({} types, {} fns) · {} edges",
        s.symbols, s.types, s.fns, s.edges
    );
    println!("  {} impl bindings", model.impls.len());

    // Spec 07: a crate with a library and a binary has two roots, and a
    // summary that does not say so reads as a census of one file.
    if s.targets.len() > 1 {
        let names: Vec<String> = s
            .targets
            .iter()
            .map(|t| format!("{}:{}", t.kind, t.name))
            .collect();
        println!("  {} targets · {}", s.targets.len(), names.join(" · "));
    }

    // Spec 03: a scan that reports structure without reporting how much of
    // it was recognised invites the reader to assume all of it was.
    if let Ok(cfg) = metatron::classify::Config::load(&dir) {
        let c = metatron::classify::classify(&model, &cfg);
        println!(
            "  coverage {}/{} symbols ({:.1}%) · {} ports",
            c.coverage.classified,
            c.coverage.total,
            c.coverage.ratio() * 100.0,
            c.coverage.ports
        );
    }

    if model.impls.is_empty() {
        println!();
        println!("  0 ports found — no trait is implemented anywhere in this crate.");
    }

    if !model.externs.is_empty() {
        println!();
        println!("  externs");
        let mut e: Vec<_> = model.externs.iter().collect();
        e.sort_by(|a, b| b.1.cmp(a.1));
        for (k, n) in e.iter().take(12) {
            println!("    {k:<28} {n}x");
        }
    }

    if !model.diagnostics.is_empty() {
        let mut by: std::collections::BTreeMap<String, Vec<&metatron::model::Diagnostic>> =
            Default::default();
        for d in &model.diagnostics {
            by.entry(format!("{:?}", d.kind)).or_default().push(d);
        }
        println!();
        println!("  {} scan diagnostics  !", model.diagnostics.len());
        for (k, v) in by {
            println!("    {k:<20} {}x", v.len());
            for d in v.iter().take(2) {
                println!("      {}:{}  {}", d.file, d.line, d.detail);
            }
        }
    }

    println!();
    println!("  -> {}", out.join("model.json").display());
    Ok(0)
}

fn cohesion(dir: &PathBuf, all: bool, json: bool) -> Result<u8> {
    use metatron::cohesion::Verdict;

    let model = metatron::scan(dir)?;
    let report = metatron::cohesion::analyse(&model);

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(0);
    }

    println!("metatron cohesion · {}", model.project);
    println!();

    let shown: Vec<_> = report
        .types
        .iter()
        .filter(|t| {
            all || !matches!(t.verdict, Verdict::Cohesive)
                || !t.unused_fields.is_empty()
                || !t.write_only_fields.is_empty()
        })
        .collect();

    if shown.is_empty() {
        println!("  no type reports a cohesion finding.");
    }

    for t in &shown {
        let tag = match t.verdict {
            Verdict::Cohesive => "cohesive",
            Verdict::Disconnected => "DISCONNECTED",
            Verdict::Splittable => "SPLITTABLE",
            Verdict::Tangled => "TANGLED",
            Verdict::Excluded => "excluded (macro)",
        };
        println!(
            "  {:<24} {}:{:<5} {:>3} fields {:>3} methods",
            t.name, t.file, t.line, t.field_count, t.method_count
        );
        println!(
            "    lcom4 {}   modularity {:.3}   {}",
            t.lcom4, t.modularity, tag
        );

        if matches!(t.verdict, Verdict::Tangled) {
            println!(
                "    no clean seam: {} components exist but share {} fields across {} accesses",
                t.components.len(),
                t.shared.len(),
                t.cross_edges
            );
        }

        for (i, c) in t.components.iter().enumerate() {
            let name = c
                .name
                .clone()
                .unwrap_or_else(|| format!("(group {})", i + 1));
            println!(
                "      {:<14} {:>3} fields {:>3} methods {:>5} loc",
                name,
                c.fields.len(),
                c.methods.len(),
                c.loc
            );
        }
        if !t.shared.is_empty() {
            println!("    shared across components: {}", t.shared.join(", "));
            println!(
                "      -> constructor parameters of each extracted type; {} cross-component accesses",
                t.cross_edges
            );
        }
        if !t.unused_fields.is_empty() {
            println!("    never touched: {}", t.unused_fields.join(", "));
        }
        if !t.write_only_fields.is_empty() {
            println!(
                "    written, never read: {}",
                t.write_only_fields.join(", ")
            );
        }
        println!();
    }

    if !report.mixed_concern.is_empty() {
        println!("  mixed-concern functions");
        for f in &report.mixed_concern {
            let cs: Vec<&str> = f.concerns.keys().map(String::as_str).collect();
            println!(
                "    {}:{:<5} {}  [{}]",
                f.file,
                f.line,
                f.symbol,
                cs.join(" + ")
            );
        }
        println!();
    }

    println!("  advisory — nothing here gates a build (spec 02).");
    Ok(0)
}

fn classify(dir: &PathBuf, verbose: bool, json: bool) -> Result<u8> {
    let model = metatron::scan(dir)?;
    let cfg = metatron::classify::Config::load(dir)?;
    let c = metatron::classify::classify(&model, &cfg);

    if json {
        println!("{}", serde_json::to_string_pretty(&c)?);
        return Ok(0);
    }

    let cov = &c.coverage;
    let pct = cov.ratio() * 100.0;
    println!("metatron classify · {}", model.project);
    println!();
    println!(
        "  coverage {}/{} symbols ({:.1}%){}",
        cov.classified,
        cov.total,
        pct,
        if cov.classified == cov.total {
            ""
        } else {
            "  !"
        }
    );

    if !cov.by_layer.is_empty() {
        println!();
        for (l, n) in &cov.by_layer {
            let pats: Vec<String> = cov
                .by_pattern
                .iter()
                .filter(|(p, _)| {
                    cfg.patterns.iter().any(|x| &x.id == *p && &x.layer == l)
                        || (*p == "infra-impl" && l == "infrastructure")
                })
                .map(|(p, n)| format!("{p} {n}"))
                .collect();
            println!("    {l:<18} {n:>4}   {}", pats.join(" · "));
        }
    }

    // The line that matters most against an unrefactored codebase. A
    // conformance tool reporting "no violations" over code it classified
    // nothing in is lying by omission.
    println!();
    if cov.ports == 0 {
        println!("  0 ports found — no trait in `domain/ports/` anywhere in this crate.");
        println!("    Every rule with a port in its premise is unevaluable, not passing.");
    } else {
        println!("  {} ports found.", cov.ports);
    }

    if !c.ambiguities.is_empty() {
        println!();
        println!("  {} ambiguous-infra", c.ambiguities.len());
        for a in &c.ambiguities {
            println!("    {a}  implements a port, names neither a persistence nor an I/O crate");
        }
    }

    if !c.unmatched.is_empty() {
        println!();
        println!(
            "  {} symbols matched no pattern. Add them to [[add_pattern]] in metatron.toml:",
            c.unmatched.len()
        );
        if verbose {
            for u in &c.unmatched {
                println!("    {}:{:<5} {:?} {}", u.file, u.line, u.kind, u.name);
            }
        } else {
            for (label, n, example) in cov.gaps.iter().take(12) {
                println!("    {label:<28} {n:>4}x   e.g. {example}");
            }
        }
    }

    let mixed = c.mixed_layer_modules(&model);
    if !mixed.is_empty() {
        println!();
        println!("  mixed-layer modules (spec 02, detector 2)");
        for (m, layers) in &mixed {
            let l: Vec<&str> = layers.iter().map(String::as_str).collect();
            println!("    {:<24} {}", m, l.join(" + "));
        }
    }

    println!();
    Ok(0)
}

fn check(
    dir: &PathBuf,
    all: bool,
    json: bool,
    only: &[String],
    allow_new: usize,
    no_fixed: bool,
) -> Result<u8> {
    use metatron::rules::{Kind, Status, Tone};

    let s = metatron::scorecard::build(dir)?;

    if json {
        #[derive(serde::Serialize)]
        struct Out<'a> {
            project: &'a str,
            coverage: f64,
            counts: metatron::scorecard::Counts,
            enforcement: metatron::scorecard::Enforcement,
            diff: &'a metatron::baseline::Diff,
            findings: &'a Vec<metatron::rules::Finding>,
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&Out {
                project: &s.model.project,
                coverage: s.coverage(),
                counts: s.counts(),
                enforcement: s.enforcement(),
                diff: &s.diff,
                findings: &s.report.findings,
            })?
        );
        return Ok(s.exit_code(allow_new) as u8);
    }

    let c = s.counts();
    let e = s.enforcement();
    let cov = &s.classified.coverage;

    println!("metatron check · {}", s.model.project);
    println!();
    // Coverage first, exactly as in metatron-nestjs and for the same
    // reason: a tool that quietly files half the code under "other" and
    // then draws a confident picture of it is worse than one that fails.
    println!(
        "  coverage    {:>6}/{} symbols ({:.1}%){}",
        cov.classified,
        cov.total,
        s.coverage(),
        if cov.classified == cov.total {
            ""
        } else {
            "  !"
        }
    );
    println!();
    println!(
        "  rules       {:>6}     upheld {} · violated {} · unevaluable {} · pass {}",
        c.rules, c.upheld, c.violated, c.unevaluable, c.passing
    );
    println!(
        "  enforcement            compiler {} · metatron {} · clippy {} · advisory {}",
        e.compiler, e.metatron, e.clippy, e.advisory
    );
    if e.compiler == 0 {
        println!("                         ^ no rule is enforced by anything but this tool");
    }

    let known = s.diff.known.len();
    let new = s.diff.new.len();
    println!();
    println!(
        "  violations  {:>6}     new {} · known {} · fixed {}",
        new + known,
        new,
        known,
        s.diff.fixed.len()
    );

    if let Some(d) = s.delta() {
        let last = s.baseline.progress.last().unwrap();
        println!(
            "  since {}         violations {:+} · coverage {:+.1}%",
            last.at, d.violations, d.coverage
        );
    }

    let flags = s.cohesion_flags();
    if !flags.is_empty() {
        println!();
        println!(
            "  cohesion    {:>6}     type(s) over threshold",
            flags.len()
        );
        for t in flags.iter().take(4) {
            println!(
                "                         {}  {}:{}  {} components, {} methods  [{:?}]",
                t.name,
                t.file,
                t.line,
                t.components.len().max(1),
                t.method_count,
                t.verdict
            );
        }
    }

    if !s.diff.new.is_empty() {
        println!();
        println!("  NEW");
        for (fp, v) in &s.diff.new {
            let loc = locate(&s, v);
            println!("    {:<22} {loc}", v.rule);
            println!("    {:<22} {fp}", "");
        }
    }

    // Spec 04: the violation the glossary cares most about is heuristic.
    // It gets reported prominently and gates nothing — which means it has
    // to appear here, not only under --all.
    let advisory: Vec<_> = s
        .report
        .findings
        .iter()
        .filter(|f| f.status == Status::Violated)
        .filter(|f| f.kind == Kind::Heuristic || !f.gate)
        .collect();
    if !advisory.is_empty() {
        println!();
        let n: usize = advisory.iter().map(|f| f.instances.len()).sum();
        println!("  ADVISORY  {n} finding(s) — reported, never gated");
        for f in &advisory {
            println!(
                "    {:<22} {:>3}  {}  [{}]",
                f.id,
                f.instances.len(),
                f.title,
                if f.kind == Kind::Heuristic {
                    "heuristic"
                } else {
                    "warn"
                }
            );
            for i in f.instances.iter().take(if all { 100 } else { 3 }) {
                println!("      {}:{:<5} {}", i.file, i.line, i.detail);
            }
            if !all && f.instances.len() > 3 {
                println!("      ... {} more", f.instances.len() - 3);
            }
        }
    }

    if !no_fixed && !s.diff.fixed.is_empty() {
        println!();
        println!("  FIXED since baseline (not removed until `metatron baseline --update`)");
        for (fp, v) in &s.diff.fixed {
            println!("    {:<22} {} -> {}  {fp}", v.rule, v.from, v.to);
        }
    }

    if all {
        println!();
        for f in &s.report.findings {
            let tag = match f.status {
                Status::Violated if f.kind == Kind::Heuristic => "heuristic",
                Status::Violated => "violated",
                Status::Unevaluable => "unevaluable",
                Status::Upheld => "upheld",
                Status::Delegated => "delegated",
                Status::Pass => "pass",
            };
            println!("  {tag:<12} {:<24} {:?}", f.id, f.tier);
            if !f.because.is_empty() {
                println!("               {}", f.because);
            }
            for i in f.instances.iter().take(6) {
                let mark = if f.tone == Tone::Warn { "!" } else { " " };
                println!("             {mark} {}:{:<5} {}", i.file, i.line, i.detail);
            }
        }
    }

    if !s.diff.excluded.is_empty() {
        println!();
        println!("  outside the ratchet");
        for x in &s.diff.excluded {
            println!("    {:<22} {}", x.rule, x.why);
        }
    }

    // `--rule` narrows what may fail, and says so rather than silently
    // shrinking the gate.
    let gating: Vec<&str> = if only.is_empty() {
        s.gating_rules()
    } else {
        s.gating_rules()
            .into_iter()
            .filter(|r| only.iter().any(|o| o == r))
            .collect()
    };

    // A name that gates nothing leaves nothing to fail, and "nothing left to
    // fail" is the PASS a typo deserves least. `metatron check --rule
    // spec-99-layerin .` used to print `gating on 0 rule(s)` and then PASS,
    // which is a green build bought with a misspelling.
    //
    // Two different mistakes land in the same place, and they deserve different
    // answers: a name no rule carries is a typo, and a name a heuristic rule
    // carries cannot fail a build by design. Neither produced a verdict, so
    // neither is a PASS — but the message should say which one it was.
    if !only.is_empty() {
        let known = s.gating_rules();
        let is_known = |o: &str| s.report.findings.iter().any(|f| f.id == o);
        let named: Vec<&str> = only.iter().map(String::as_str).collect();
        let unknown: Vec<&str> = named.iter().copied().filter(|o| !is_known(o)).collect();
        let advisory: Vec<&str> = named
            .iter()
            .copied()
            .filter(|o| is_known(o) && !known.contains(o))
            .collect();

        if !unknown.is_empty() {
            anyhow::bail!(
                "--rule matched no rule in this crate: {}\n  the rules here are: {}",
                unknown.join(", "),
                s.report
                    .findings
                    .iter()
                    .map(|f| f.id)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if !advisory.is_empty() {
            anyhow::bail!(
                "--rule named {}, which is advisory and cannot fail a build\n  \
                 gating rules are: {}",
                advisory.join(", "),
                if known.is_empty() {
                    "none in this crate".to_string()
                } else {
                    known.join(", ")
                }
            );
        }
    }
    let failing = s
        .diff
        .new
        .iter()
        .filter(|(_, v)| gating.contains(&v.rule.as_str()))
        .count();

    println!();
    if !only.is_empty() {
        println!(
            "  gating on {} rule(s): {}",
            gating.len(),
            gating.join(", ")
        );
    }
    if !s.had_baseline {
        println!(
            "  no {} — run `metatron baseline --update` to accept the current state.",
            metatron::baseline::FILE
        );
    }
    if failing > allow_new {
        println!("  FAIL — {failing} new violation(s), {allow_new} allowed.");
        return Ok(REGRESSION);
    }

    // A verdict is only as good as what it looked at. `PASS` on a line by
    // itself is a claim about the whole crate, and this tool knows three things
    // that make the claim narrower: how much of the code was classified at all,
    // how many rules could not decide, and whether there was a known state to
    // compare against. They belong beside the word, not in a footnote nobody
    // scrolls to.
    //
    // The exit code is unchanged here. Making these gate is a separate,
    // opt-in decision (`--min-coverage`, `--require-evaluable`); this commit
    // only stops the text from claiming more than it measured.
    let mut narrower = Vec::new();
    if !s.had_baseline {
        narrower.push(format!(
            "no {}, so 'new' means 'all' and nothing was compared",
            metatron::baseline::FILE
        ));
    }
    if cov.classified < cov.total {
        narrower.push(format!(
            "{}/{} symbols classified ({:.1}%) — the rest are outside the model",
            cov.classified,
            cov.total,
            s.coverage()
        ));
    }
    if c.unevaluable > 0 {
        narrower.push(format!(
            "{} rule(s) unevaluable — a rule that cannot decide did not pass",
            c.unevaluable
        ));
    }
    if !gating.is_empty() && gating.len() < s.gating_rules().len() {
        narrower.push(format!(
            "gating narrowed to {} of {} rule(s)",
            gating.len(),
            s.gating_rules().len()
        ));
    }

    println!(
        "  PASS — no new violations. {} known. {}",
        s.diff.known.len() + if s.had_baseline { 0 } else { s.diff.new.len() },
        if narrower.is_empty() {
            String::from("")
        } else {
            format!("(note: {})", narrower.join("; "))
        }
    );
    Ok(0)
}

fn locate(s: &metatron::Scorecard, v: &metatron::baseline::Entry) -> String {
    s.report
        .findings
        .iter()
        .find(|f| f.id == v.rule)
        .and_then(|f| {
            f.instances
                .iter()
                .find(|i| i.from == v.from && i.to == v.to)
        })
        .map(|i| format!("{}:{:<5} {}", i.file, i.line, i.detail))
        .unwrap_or_else(|| format!("{} -> {}", v.from, v.to))
}

fn baseline_cmd(dir: &PathBuf, write: bool) -> Result<u8> {
    let s = metatron::scorecard::build(dir)?;
    let (next, out) =
        metatron::baseline::update(&s.report, &s.baseline, &s.model.project, s.coverage());

    println!("metatron baseline · {}", s.model.project);
    println!();
    println!("  {} violation(s) accepted", out.written);
    println!("  {} note(s) preserved", out.notes_preserved);
    if !out.notes_dropped.is_empty() {
        // Reported separately from preserved, with an accurate count.
        // metatron-nestjs shipped a bug here: it reported "1 note
        // preserved" while preserving none.
        println!(
            "  {} note(s) dropped — their violation no longer exists:",
            out.notes_dropped.len()
        );
        for (fp, e) in &out.notes_dropped {
            println!("    {fp}  {:<22} {}", e.rule, e.note);
        }
    }

    if !s.diff.excluded.is_empty() {
        println!();
        println!("  not ratcheted");
        for x in &s.diff.excluded {
            println!("    {:<22} {}", x.rule, x.why);
        }
    }

    println!();
    if write {
        next.save(dir)?;
        println!("  -> {}", metatron::baseline::path_of(dir).display());
        if let Some(p) = next.progress.last() {
            println!(
                "  progress: {} entries, latest {} ({} violations, {:.1}% coverage)",
                next.progress.len(),
                p.at,
                p.violations,
                p.coverage
            );
        }
    } else {
        println!(
            "  dry run — pass --update to write {}",
            metatron::baseline::FILE
        );
    }
    Ok(0)
}

fn views(dir: &PathBuf, name: Option<&str>, out: Option<PathBuf>) -> Result<u8> {
    let s = metatron::scorecard::build(dir)?;
    let out = out.unwrap_or_else(|| dir.join(".metatron/views"));

    if let Some(n) = name {
        let html = metatron::views::render(&s, n)?;
        std::fs::create_dir_all(&out)?;
        let p = out.join(format!("{n}.html"));
        std::fs::write(&p, html)?;
        println!("metatron views · {}", s.model.project);
        println!("  -> {}", p.display());
        return Ok(0);
    }

    let written = metatron::views::write_all(&s, &out)?;
    println!("metatron views · {}", s.model.project);
    println!();
    for v in metatron::views::VIEWS {
        let on = written.iter().any(|w| w == v.name);
        println!(
            "  {} {:<10} {}",
            if on { "*" } else { " " },
            v.name,
            if on {
                v.blurb
            } else {
                "omitted — nothing to say about this crate"
            }
        );
    }
    // Omitted, not emptied: metatron's build drops narration slots with
    // nothing to say, and the same instinct applies to a whole lens.
    println!("    schema     omitted — no ORM in Rust; revisit when a domain layer exists");
    println!();
    println!("  -> {}", out.join("index.html").display());
    Ok(0)
}
