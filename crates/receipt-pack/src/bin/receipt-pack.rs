//! `receipt-pack`: build a context pack for a task, or verify an earlier pack.
//!
//! `build` prints the pack (Markdown or JSON) on stdout, or writes it with `--out`.
//! `verify` re-hashes every item in a JSON pack. Exit codes: 0 ok (verify: all fresh),
//! 1 stale or missing items (verify only), 3 bad input, 64 usage error.

use receipt_pack::{build, markdown, parse_pack, verify, Kind, KindRules, Options, Status};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
usage: receipt-pack build --task TEXT --root DIR [options]
       receipt-pack verify PACK.json [--root DIR] [--format text|json]

build: walks DIR for Markdown and text notes, scores every section against
the task (BM25, kind weight, recency, tag match; no model, no network) and
prints the best items that fit the token budget, each with its path, line
range, sha256 and git commit. Anti-patterns that match the task are printed
first, in a WARNING section, even when they exceed the budget.

verify: re-reads every item of a JSON pack and reports FRESH, STALE or
MISSING. Paths are resolved against the pack's root unless --root is given.

Exit codes: 0 ok (verify: every item fresh), 1 stale or missing items,
3 bad input (no task terms, bad root, unreadable pack), 64 usage error.

build options:
  --task TEXT            what you are about to do (required)
  --root DIR             directory of notes (required)
  --budget-tokens N      token budget for all excerpts (default 2000)
  --format md|json       output format (default md; verify needs json)
  --out PATH             write the pack to PATH instead of stdout
  --compact              JSON on one line
  --kind-path KIND=GLOB  paths matching GLOB get KIND (repeatable, tried
                         before the defaults)
  --kind-tag KIND=TAG    files tagged TAG get KIND (repeatable)
  --no-default-kinds     drop the default path and tag rules
  --exclude GLOB         skip matching paths (repeatable)
  --ext EXT              index files with this extension (repeatable;
                         default md, markdown, txt)
  --max-warnings N       most anti-patterns in the WARNING section (default 3)
  --warn-min-terms N     task terms an anti-pattern must match (default 2)
  --max-item-lines N     longest item before a section is cut (default 40)
  --half-life-days N     recency half-life (default 30)
  --now YYYY-MM-DD       date to measure recency from (default today, UTC)
  --no-git               do not look up git commits

kinds: rule, decision, fact, anti-pattern, evidence
default kind rules: memory/learnings/incidents/** and tags anti-pattern or
incident are anti-patterns; memory/tasks/*.md are evidence; **/decisions/**
are decisions; Always/Never/Prefer bullets are rules; the rest are facts.
Frontmatter type: wins over every rule.

  -h, --help             show this help
  -V, --version          show the version";

enum Cmd {
    Build {
        opts: Options,
        json: bool,
        compact: bool,
        out: Option<PathBuf>,
    },
    Verify {
        pack: PathBuf,
        root: Option<PathBuf>,
        json: bool,
        compact: bool,
    },
    Exit(ExitCode),
}

fn number<T: std::str::FromStr>(name: &str, v: &str) -> Result<T, String> {
    v.parse()
        .map_err(|_| format!("{name} needs a number, got {v:?}"))
}

fn kind_pair(name: &str, v: &str) -> Result<(String, Kind), String> {
    let (k, rest) = v
        .split_once('=')
        .ok_or_else(|| format!("{name} needs KIND=VALUE, got {v:?}"))?;
    let kind = Kind::parse(k).ok_or_else(|| format!("{name}: unknown kind {k:?}"))?;
    if rest.is_empty() {
        return Err(format!("{name}: empty value in {v:?}"));
    }
    Ok((rest.to_owned(), kind))
}

fn format_flag(v: &str, allowed: &[&str]) -> Result<bool, String> {
    if !allowed.contains(&v) {
        return Err(format!("--format must be one of {}", allowed.join(", ")));
    }
    Ok(v == "json")
}

fn parse(argv: Vec<String>) -> Result<Cmd, String> {
    let mut it = argv.into_iter();
    let sub = it.next().ok_or("a command is required: build or verify")?;
    match sub.as_str() {
        "-h" | "--help" => {
            println!("{USAGE}");
            return Ok(Cmd::Exit(ExitCode::SUCCESS));
        }
        "-V" | "--version" => {
            println!("{}", receipt_pack::TOOL);
            return Ok(Cmd::Exit(ExitCode::SUCCESS));
        }
        "build" | "verify" => {}
        other => return Err(format!("unknown command: {other}")),
    }
    let mut task = None;
    let mut root: Option<PathBuf> = None;
    let mut pack = None;
    let mut json = false;
    let mut compact = false;
    let mut out = None;
    let mut user_paths = Vec::new();
    let mut user_tags = Vec::new();
    let mut defaults = true;
    let mut exclude = Vec::new();
    let mut exts = Vec::new();
    let mut opts = Options::new("", "");
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match (sub.as_str(), arg.as_str()) {
            (_, "-h" | "--help") => {
                println!("{USAGE}");
                return Ok(Cmd::Exit(ExitCode::SUCCESS));
            }
            (_, "--root") => root = Some(value("--root")?.into()),
            (_, "--compact") => compact = true,
            ("build", "--format") => {
                json = format_flag(&value("--format")?, &["md", "json"])?;
            }
            ("verify", "--format") => {
                json = format_flag(&value("--format")?, &["text", "json"])?;
            }
            ("build", "--task") => task = Some(value("--task")?),
            ("build", "--budget-tokens") => {
                opts.budget_tokens = number("--budget-tokens", &value("--budget-tokens")?)?
            }
            ("build", "--out") => out = Some(PathBuf::from(value("--out")?)),
            ("build", "--kind-path") => {
                user_paths.push(kind_pair("--kind-path", &value("--kind-path")?)?)
            }
            ("build", "--kind-tag") => {
                user_tags.push(kind_pair("--kind-tag", &value("--kind-tag")?)?)
            }
            ("build", "--no-default-kinds") => defaults = false,
            ("build", "--exclude") => exclude.push(value("--exclude")?),
            ("build", "--ext") => exts.push(value("--ext")?.trim_start_matches('.').to_owned()),
            ("build", "--max-warnings") => {
                opts.max_warnings = number("--max-warnings", &value("--max-warnings")?)?
            }
            ("build", "--warn-min-terms") => {
                opts.warn_min_terms = number("--warn-min-terms", &value("--warn-min-terms")?)?
            }
            ("build", "--max-item-lines") => {
                opts.max_item_lines = number("--max-item-lines", &value("--max-item-lines")?)?
            }
            ("build", "--half-life-days") => {
                let d: f64 = number("--half-life-days", &value("--half-life-days")?)?;
                if !(d > 0.0 && d.is_finite()) {
                    return Err("--half-life-days must be above 0".into());
                }
                opts.half_life_days = d;
            }
            ("build", "--now") => opts.now = Some(value("--now")?),
            ("build", "--no-git") => opts.git = false,
            ("verify", a) if !a.starts_with('-') && pack.is_none() => pack = Some(a.into()),
            (_, other) => return Err(format!("unknown argument for {sub}: {other}")),
        }
    }

    if sub == "verify" {
        let pack = pack.ok_or("verify needs a PACK.json path")?;
        return Ok(Cmd::Verify {
            pack,
            root,
            json,
            compact,
        });
    }
    opts.task = task.ok_or("--task is required")?;
    opts.root = root.ok_or("--root is required")?;
    let mut rules = if defaults {
        KindRules::default()
    } else {
        KindRules::empty()
    };
    user_paths.extend(rules.paths);
    user_tags.extend(rules.tags);
    rules.paths = user_paths;
    rules.tags = user_tags;
    opts.rules = rules;
    opts.exclude = exclude;
    if !exts.is_empty() {
        opts.extensions = exts;
    }
    Ok(Cmd::Build {
        opts,
        json,
        compact,
        out,
    })
}

fn to_json<T: serde::Serialize>(value: &T, compact: bool) -> String {
    let out = if compact {
        serde_json::to_string(value)
    } else {
        serde_json::to_string_pretty(value)
    };
    out.expect("pack serializes")
}

fn run_build(opts: Options, json: bool, compact: bool, out: Option<PathBuf>) -> ExitCode {
    let pack = match build(&opts) {
        Ok(p) => p,
        Err(err) => {
            println!("{}", to_json(&err, compact));
            eprintln!("receipt-pack: {err}");
            return ExitCode::from(3);
        }
    };
    let text = if json {
        to_json(&pack, compact) + "\n"
    } else {
        markdown(&pack)
    };
    match out {
        None => print!("{text}"),
        Some(path) => {
            if let Err(e) = std::fs::write(&path, text) {
                eprintln!("receipt-pack: {}: {e}", path.display());
                return ExitCode::from(3);
            }
            let c = &pack.counts;
            eprintln!(
                "receipt-pack: wrote {} ({} warnings, {} items, {} of {} tokens)",
                path.display(),
                c.warnings,
                c.included,
                pack.budget.used,
                pack.budget.tokens
            );
        }
    }
    ExitCode::SUCCESS
}

fn run_verify(path: PathBuf, root: Option<PathBuf>, json: bool, compact: bool) -> ExitCode {
    let parsed = std::fs::read_to_string(&path)
        .map_err(|e| format!("{}: {e}", path.display()))
        .and_then(|s| parse_pack(&s).map_err(|e| format!("{}: {e}", path.display())));
    let pack = match parsed {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("receipt-pack: {msg}");
            return ExitCode::from(3);
        }
    };
    let root = root.unwrap_or_else(|| PathBuf::from(&pack.root));
    let report = verify(&pack, &root);
    if json {
        println!("{}", to_json(&report, compact));
    } else {
        for c in &report.items {
            let status = match c.status {
                Status::Fresh => "FRESH",
                Status::Stale => "STALE",
                Status::Missing => "MISSING",
            };
            let detail = c
                .detail
                .as_deref()
                .map(|d| format!("  ({d})"))
                .unwrap_or_default();
            println!(
                "{status:<8} {}:{}-{}{detail}",
                c.path, c.lines.start, c.lines.end
            );
        }
        println!(
            "{} items: {} fresh, {} stale, {} missing",
            report.checked, report.fresh, report.stale, report.missing
        );
    }
    if report.ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1).collect()) {
        Ok(Cmd::Exit(code)) => code,
        Ok(Cmd::Build {
            opts,
            json,
            compact,
            out,
        }) => run_build(opts, json, compact, out),
        Ok(Cmd::Verify {
            pack,
            root,
            json,
            compact,
        }) => run_verify(pack, root, json, compact),
        Err(msg) => {
            eprintln!("receipt-pack: {msg}\n\n{USAGE}");
            ExitCode::from(64)
        }
    }
}
