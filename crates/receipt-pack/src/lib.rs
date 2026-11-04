//! Token-capped context packs with provenance.
//!
//! Before a task, [`build`] walks a directory of notes (Markdown and plain text), splits
//! every file into items (sections, long sections in pieces, and each `Always` / `Never` /
//! `Prefer` bullet on its own), gives each item a [`Kind`], scores it against the task and
//! keeps the best items that fit a token budget. Every item points to its evidence: the path
//! relative to the root, the line range, the SHA-256 of the excerpt and, in a git work tree,
//! the last commit that touched the file. [`verify`] re-reads the files later and reports
//! every item as fresh, stale or missing.
//!
//! Anti-patterns (recorded mistakes) that match the task go into a separate warning list
//! that is filled before the budget is applied, so they are never cut for space.
//!
//! Every item carries its score breakdown:
//!
//! ```text
//! total = (lexical + tag_match + semantic) x kind_weight x recency
//! ```
//!
//! `lexical` is BM25 over the task terms, `tag_match` is [`TAG_WEIGHT`] per frontmatter tag
//! that matches a task term (at most [`MAX_TAG_MATCHES`]), `kind_weight` comes from
//! [`Kind::weight`] and `recency` falls from 1.0 towards 0.5 with a configurable half-life.
//! `semantic` is always absent in this version: there are no embeddings, no model calls and
//! no network. [`SemanticScorer`] is the hook for a later local-embedding scorer.

mod classify;
mod date;
mod git;
mod render;
mod scan;
mod text;

pub use classify::{glob_match, Kind, KindRules};
pub use render::markdown;
pub use text::{stem, task_terms, Term};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};

/// Identifies this tool in every pack and report.
pub const TOOL: &str = concat!("receipt-pack ", env!("CARGO_PKG_VERSION"));
pub const DEFAULT_BUDGET_TOKENS: usize = 2000;
/// Score added per frontmatter tag that matches a task term.
pub const TAG_WEIGHT: f64 = 1.0;
/// At most this many matching tags add to the score (all are listed).
pub const MAX_TAG_MATCHES: usize = 3;
/// A [`SemanticScorer`] similarity of 1.0 adds this much to the score.
pub const SEMANTIC_WEIGHT: f64 = 2.0;
/// Recency never takes away more than half of an item's score.
pub const RECENCY_FLOOR: f64 = 0.5;

/// Hook for a scorer that compares meaning rather than words, such as a local embedding
/// model. This version ships none: [`build`] passes `None` to [`build_with`].
///
/// An implementation must be deterministic and local (no network) to keep the pack
/// reproducible. Its contribution is `similarity x SEMANTIC_WEIGHT`, added to the lexical and
/// tag scores before the kind and recency factors, and shown as `score.semantic`. With a
/// scorer, an item with no matching terms or tags is still a candidate if its similarity is
/// above zero.
pub trait SemanticScorer {
    /// Short identifier recorded with every score, e.g. `"local-minilm-l6"`.
    fn name(&self) -> &str;
    /// Similarity of the task and an item's text (heading and excerpt), in `[0, 1]`.
    /// Values outside the range are clamped.
    fn similarity(&self, task: &str, text: &str) -> f64;
}

/// What to build a pack from. [`Options::new`] gives the defaults.
#[derive(Debug, Clone)]
pub struct Options {
    pub task: String,
    pub root: PathBuf,
    /// Token budget for all excerpts, warnings included. Tokens are estimated as
    /// `ceil(characters / 4)` of the excerpt.
    pub budget_tokens: usize,
    pub rules: KindRules,
    /// Globs of root-relative paths to skip, e.g. `memory/logs/**`.
    pub exclude: Vec<String>,
    /// File extensions to index, without the dot.
    pub extensions: Vec<String>,
    /// Most anti-patterns placed in the WARNING section.
    pub max_warnings: usize,
    /// An anti-pattern becomes a warning when it matches at least this many task terms
    /// (capped at the number of task terms) or any of its tags matches.
    pub warn_min_terms: usize,
    /// Longest item, in lines; longer sections are cut into pieces.
    pub max_item_lines: usize,
    /// Files larger than this are skipped.
    pub max_file_bytes: u64,
    /// Age at which recency has fallen halfway from 1.0 to [`RECENCY_FLOOR`].
    pub half_life_days: f64,
    /// "Today" for recency and the `generated` date, as `YYYY-MM-DD`. Default: the UTC date.
    pub now: Option<String>,
    /// Look up git commits when the root is in a git work tree.
    pub git: bool,
}

impl Options {
    pub fn new(task: impl Into<String>, root: impl Into<PathBuf>) -> Self {
        Options {
            task: task.into(),
            root: root.into(),
            budget_tokens: DEFAULT_BUDGET_TOKENS,
            rules: KindRules::default(),
            exclude: Vec::new(),
            extensions: vec!["md".into(), "markdown".into(), "txt".into()],
            max_warnings: 3,
            warn_min_terms: 2,
            max_item_lines: 40,
            max_file_bytes: 1 << 20,
            half_life_days: 30.0,
            now: None,
            git: true,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lines {
    /// First line, 1-based.
    pub start: usize,
    /// Last line, inclusive.
    pub end: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Lexical {
    /// BM25 over the task terms (k1 1.2, b 0.75); 0 when no term matches.
    pub score: f64,
    /// Task terms found in the item (its path, heading or text), in task order.
    pub matched_terms: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Recency {
    /// `0.5 + 0.5 x 2^(-age_days / half_life)`; 0.5 when the date is unknown.
    pub factor: f64,
    pub date: Option<String>,
    pub age_days: Option<i64>,
    /// `frontmatter:<key>`, `filename`, `mtime` or `none`.
    pub source: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TagMatch {
    pub score: f64,
    /// Frontmatter tags with a word that matches a task term.
    pub matched_tags: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Semantic {
    pub scorer: String,
    pub similarity: f64,
    pub score: f64,
}

/// Why an item came up. Every number is rounded to 4 decimal places, and `total` is
/// computed from the rounded parts.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Score {
    pub lexical: Lexical,
    pub kind_weight: f64,
    pub recency: Recency,
    pub tag_match: TagMatch,
    /// Always `null` in this version; see [`SemanticScorer`].
    pub semantic: Option<Semantic>,
    pub total: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Item {
    /// `path#Lstart-Lend`.
    pub id: String,
    pub kind: Kind,
    /// Which rule set the kind: `frontmatter:type`, `tag:<tag>`, `path:<glob>`,
    /// `bullet:always-never-prefer` or `default`.
    pub kind_reason: String,
    /// Relative to the root, `/`-separated.
    pub path: String,
    pub lines: Lines,
    pub heading: Option<String>,
    pub tags: Vec<String>,
    /// SHA-256 (hex) of the excerpt: the lines without terminators, joined with `\n`.
    pub sha256: String,
    /// Last commit that touched the file; `null` outside git or when never committed.
    pub git_commit: Option<String>,
    /// The file differs from HEAD or is untracked; `null` outside git.
    pub git_modified: Option<bool>,
    /// Estimated tokens of the excerpt.
    pub tokens: usize,
    pub score: Score,
    pub excerpt: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub tokens: usize,
    /// Tokens of every excerpt in the pack, warnings included.
    pub used: usize,
    pub warning_tokens: usize,
    /// The warnings alone exceed the budget. They are kept anyway.
    pub over_budget: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    /// Files indexed.
    pub files: usize,
    /// Items in those files.
    pub items: usize,
    /// Items with a score above zero.
    pub candidates: usize,
    pub warnings: usize,
    pub included: usize,
    /// Candidates that did not fit the budget.
    pub dropped_for_budget: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Pack {
    pub tool: String,
    pub task: String,
    /// Absolute root the paths are relative to.
    pub root: String,
    /// The date used as "today", `YYYY-MM-DD`.
    pub generated: String,
    pub git_head: Option<String>,
    /// Task terms as written, lowercased, in task order.
    pub terms: Vec<String>,
    pub budget: Budget,
    pub counts: Counts,
    /// Anti-patterns that match the task, best first. Never cut for budget.
    pub warnings: Vec<Item>,
    /// Everything else that fit, best first.
    pub items: Vec<Item>,
    /// Always false: this crate never calls a model.
    pub model_called: bool,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    TaskRequired,
    NoTerms,
    BadRoot,
    BadDate,
    BadPack,
}

/// Bad input: no task, a root that is not a directory, an unreadable pack.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Error {
    pub code: ErrorCode,
    pub error: String,
}

impl Error {
    fn new(code: ErrorCode, error: impl Into<String>) -> Self {
        Error {
            code,
            error: error.into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.error)
    }
}

impl std::error::Error for Error {}

pub fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Tokens of `text`, estimated as `ceil(characters / 4)`, at least 1.
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4).max(1)
}

/// Lines `start..=end` (1-based) of `content`, joined with `\n`; `None` if out of range.
pub fn excerpt(content: &str, start: usize, end: usize) -> Option<String> {
    let lines = scan::split_lines(content);
    if start == 0 || end < start || end > lines.len() {
        return None;
    }
    Some(lines[start - 1..end].join("\n"))
}

pub fn recency_factor(age_days: i64, half_life_days: f64) -> f64 {
    let age = age_days.max(0) as f64;
    RECENCY_FLOOR + (1.0 - RECENCY_FLOOR) * 0.5f64.powf(age / half_life_days.max(f64::MIN_POSITIVE))
}

fn round4(x: f64) -> f64 {
    (x * 10_000.0).round() / 10_000.0
}

fn file_recency(
    fm: &scan::Frontmatter,
    rel: &str,
    mtime: Option<i64>,
    now: i64,
    half_life: f64,
) -> Recency {
    let (day, source) = if let Some((key, d)) = &fm.date {
        (Some(*d), format!("frontmatter:{key}"))
    } else if let Some(d) = scan::filename_date(rel) {
        (Some(d), "filename".to_owned())
    } else if let Some(d) = mtime {
        (Some(d), "mtime".to_owned())
    } else {
        (None, "none".to_owned())
    };
    match day {
        Some(d) => {
            let age = (now - d).max(0);
            Recency {
                factor: round4(recency_factor(age, half_life)),
                date: Some(date::format_date(d)),
                age_days: Some(age),
                source,
            }
        }
        None => Recency {
            factor: RECENCY_FLOOR,
            date: None,
            age_days: None,
            source,
        },
    }
}

/// An item before scoring.
struct Draft {
    path: String,
    lines: Lines,
    heading: Option<String>,
    kind: Kind,
    kind_reason: String,
    tags: Vec<String>,
    recency: Recency,
    excerpt: String,
}

fn path_words(rel: &str) -> &str {
    match rel.rsplit_once('.') {
        Some((stem, ext)) if !ext.contains('/') => stem,
        _ => rel,
    }
}

/// Build a pack with lexical scoring only. See [`build_with`].
pub fn build(opts: &Options) -> Result<Pack, Error> {
    build_with(opts, None)
}

/// Build a pack, optionally adding a [`SemanticScorer`] to the lexical score.
///
/// Items are ordered by total score (highest first), then path, then first line, so the
/// same files and options always give the same pack.
pub fn build_with(opts: &Options, semantic: Option<&dyn SemanticScorer>) -> Result<Pack, Error> {
    let task = opts.task.trim();
    if task.is_empty() {
        return Err(Error::new(ErrorCode::TaskRequired, "task is required"));
    }
    let terms = task_terms(task);
    if terms.is_empty() {
        return Err(Error::new(
            ErrorCode::NoTerms,
            "task has no searchable terms (only stop words or numbers)",
        ));
    }
    let now = match &opts.now {
        Some(s) => date::parse_date(s)
            .ok_or_else(|| Error::new(ErrorCode::BadDate, format!("not a YYYY-MM-DD date: {s}")))?,
        None => date::today(),
    };
    let root = opts
        .root
        .canonicalize()
        .ok()
        .filter(|r| r.is_dir())
        .ok_or_else(|| {
            Error::new(
                ErrorCode::BadRoot,
                format!("root is not a directory: {}", opts.root.display()),
            )
        })?;

    let files = scan::walk(&root, &opts.exclude, &opts.extensions, opts.max_file_bytes);
    let mut drafts = Vec::new();
    let mut docs = Vec::new();
    let mut indexed = 0;
    for rel in &files {
        let path = root.join(rel);
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        indexed += 1;
        let mtime = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(date::from_system_time);
        let lines = scan::split_lines(&content);
        let fm = scan::parse_frontmatter(&lines);
        let recency = file_recency(&fm, rel, mtime, now, opts.half_life_days);
        let path_terms = text::terms(path_words(rel));
        for c in scan::chunk(&lines, fm.body_start, opts.max_item_lines) {
            let (kind, kind_reason) = opts.rules.classify(rel, &fm, c.rule_bullet);
            let excerpt = lines[c.start - 1..c.end].join("\n");
            let mut doc = path_terms.clone();
            if let Some(h) = &c.heading {
                doc.extend(text::terms(h));
            }
            doc.extend(text::terms(&excerpt));
            docs.push(doc);
            drafts.push(Draft {
                path: rel.clone(),
                lines: Lines {
                    start: c.start,
                    end: c.end,
                },
                heading: c.heading,
                kind,
                kind_reason,
                tags: fm.tags.clone(),
                recency: recency.clone(),
                excerpt,
            });
        }
    }

    let stems: Vec<String> = terms.iter().map(|t| t.stem.clone()).collect();
    let stem_set: BTreeSet<&str> = stems.iter().map(String::as_str).collect();
    let item_count = drafts.len();
    let mut scored = Vec::new();
    for (d, (lex, matched)) in drafts.into_iter().zip(text::bm25(&stems, &docs)) {
        let matched_tags: Vec<String> = d
            .tags
            .iter()
            .filter(|t| text::terms(t).iter().any(|s| stem_set.contains(s.as_str())))
            .cloned()
            .collect();
        let tag_score = TAG_WEIGHT * matched_tags.len().min(MAX_TAG_MATCHES) as f64;
        let semantic = semantic.map(|s| {
            let text = format!("{}\n{}", d.heading.as_deref().unwrap_or(""), d.excerpt);
            let sim = round4(s.similarity(task, &text).clamp(0.0, 1.0));
            Semantic {
                scorer: s.name().to_owned(),
                similarity: sim,
                score: round4(sim * SEMANTIC_WEIGHT),
            }
        });
        let sem_score = semantic.as_ref().map_or(0.0, |s| s.score);
        let lex = round4(lex);
        if lex <= 0.0 && tag_score <= 0.0 && sem_score <= 0.0 {
            continue;
        }
        let kind_weight = d.kind.weight();
        let total = round4((lex + tag_score + sem_score) * kind_weight * d.recency.factor);
        scored.push(Item {
            id: format!("{}#L{}-L{}", d.path, d.lines.start, d.lines.end),
            kind: d.kind,
            kind_reason: d.kind_reason,
            sha256: sha256_hex(&d.excerpt),
            tokens: estimate_tokens(&d.excerpt),
            path: d.path,
            lines: d.lines,
            heading: d.heading,
            tags: d.tags,
            git_commit: None,
            git_modified: None,
            score: Score {
                lexical: Lexical {
                    score: lex,
                    matched_terms: matched.iter().map(|&i| terms[i].word.clone()).collect(),
                },
                kind_weight,
                recency: d.recency,
                tag_match: TagMatch {
                    score: tag_score,
                    matched_tags,
                },
                semantic,
                total,
            },
            excerpt: d.excerpt,
        });
    }
    scored.sort_by(|a, b| {
        b.score
            .total
            .total_cmp(&a.score.total)
            .then_with(|| a.path.cmp(&b.path))
            .then(a.lines.start.cmp(&b.lines.start))
    });
    let candidates = scored.len();

    let need = opts.warn_min_terms.min(terms.len()).max(1);
    let mut warnings = Vec::new();
    let mut rest = Vec::new();
    for it in scored {
        let matches = it.score.lexical.matched_terms.len() >= need
            || !it.score.tag_match.matched_tags.is_empty();
        if it.kind == Kind::AntiPattern && matches && warnings.len() < opts.max_warnings {
            warnings.push(it);
        } else {
            rest.push(it);
        }
    }
    let warning_tokens: usize = warnings.iter().map(|i| i.tokens).sum();
    let mut used = warning_tokens;
    let mut items = Vec::new();
    let mut dropped = 0;
    for it in rest {
        if used + it.tokens <= opts.budget_tokens {
            used += it.tokens;
            items.push(it);
        } else {
            dropped += 1;
        }
    }

    let mut repo = if opts.git {
        git::Repo::open(&root)
    } else {
        None
    };
    let git_head = repo.as_ref().and_then(|r| r.head.clone());
    if let Some(repo) = repo.as_mut() {
        for it in warnings.iter_mut().chain(items.iter_mut()) {
            it.git_commit = repo.commit(&it.path);
            it.git_modified = Some(repo.modified(&it.path));
        }
    }

    Ok(Pack {
        tool: TOOL.to_owned(),
        task: task.to_owned(),
        root: root.to_string_lossy().into_owned(),
        generated: date::format_date(now),
        git_head,
        terms: terms.into_iter().map(|t| t.word).collect(),
        budget: Budget {
            tokens: opts.budget_tokens,
            used,
            warning_tokens,
            over_budget: warning_tokens > opts.budget_tokens,
        },
        counts: Counts {
            files: indexed,
            items: item_count,
            candidates,
            warnings: warnings.len(),
            included: items.len(),
            dropped_for_budget: dropped,
        },
        warnings,
        items,
        model_called: false,
    })
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "UPPERCASE")]
pub enum Status {
    /// The excerpt hashes to the recorded SHA-256.
    Fresh,
    /// The file is there but the lines changed, or the file is now shorter than the range.
    Stale,
    /// The file is gone or unreadable, or the recorded path is not under the root.
    Missing,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub id: String,
    pub path: String,
    pub lines: Lines,
    pub status: Status,
    pub expected_sha256: String,
    pub actual_sha256: Option<String>,
    pub detail: Option<String>,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct VerifyReport {
    pub tool: &'static str,
    pub root: String,
    pub checked: usize,
    pub fresh: usize,
    pub stale: usize,
    pub missing: usize,
    /// Warnings first, then items, in pack order.
    pub items: Vec<Check>,
}

impl VerifyReport {
    /// Every item is fresh.
    pub fn ok(&self) -> bool {
        self.stale == 0 && self.missing == 0
    }
}

/// A path from a pack is usable only if it stays under the root.
fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

/// Parse a pack written by `receipt-pack build --format json`.
pub fn parse_pack(json: &str) -> Result<Pack, Error> {
    serde_json::from_str(json)
        .map_err(|e| Error::new(ErrorCode::BadPack, format!("not a pack: {e}")))
}

/// Re-read every item's lines under `root` and compare their SHA-256 with the pack.
pub fn verify(pack: &Pack, root: &Path) -> VerifyReport {
    let mut files: HashMap<&str, Result<String, String>> = HashMap::new();
    let mut checks = Vec::new();
    for it in pack.warnings.iter().chain(&pack.items) {
        let (status, actual, detail) = if !safe_relative(&it.path) {
            (
                Status::Missing,
                None,
                Some("path is not relative to the root".to_owned()),
            )
        } else {
            let content = files.entry(it.path.as_str()).or_insert_with(|| {
                std::fs::read(root.join(&it.path))
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .map_err(|e| e.to_string())
            });
            match content {
                Err(e) => (Status::Missing, None, Some(e.clone())),
                Ok(c) => match excerpt(c, it.lines.start, it.lines.end) {
                    None => (
                        Status::Stale,
                        None,
                        Some(format!(
                            "line range past end of file ({} lines)",
                            scan::split_lines(c).len()
                        )),
                    ),
                    Some(text) => {
                        let h = sha256_hex(&text);
                        let status = if h == it.sha256 {
                            Status::Fresh
                        } else {
                            Status::Stale
                        };
                        (status, Some(h), None)
                    }
                },
            }
        };
        checks.push(Check {
            id: it.id.clone(),
            path: it.path.clone(),
            lines: it.lines,
            status,
            expected_sha256: it.sha256.clone(),
            actual_sha256: actual,
            detail,
        });
    }
    let count = |s| checks.iter().filter(|c| c.status == s).count();
    VerifyReport {
        tool: TOOL,
        root: root.to_string_lossy().into_owned(),
        checked: checks.len(),
        fresh: count(Status::Fresh),
        stale: count(Status::Stale),
        missing: count(Status::Missing),
        items: checks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("receipt-pack-unit-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn opts(task: &str, root: &Path) -> Options {
        let mut o = Options::new(task, root);
        o.now = Some("2026-06-01".into());
        o.git = false;
        o
    }

    #[test]
    fn hashes_and_excerpts() {
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(excerpt("a\r\nb\nc", 2, 3).as_deref(), Some("b\nc"));
        assert_eq!(excerpt("a\nb", 2, 3), None);
        assert_eq!(excerpt("a\nb", 0, 1), None);
        assert_eq!(estimate_tokens(""), 1);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    #[test]
    fn recency_halves_towards_floor() {
        assert_eq!(recency_factor(0, 30.0), 1.0);
        assert_eq!(recency_factor(-5, 30.0), 1.0);
        assert!((recency_factor(30, 30.0) - 0.75).abs() < 1e-12);
        assert!(recency_factor(10_000, 30.0) - RECENCY_FLOOR < 1e-9);
    }

    #[test]
    fn errors_for_bad_input() {
        let d = tmp("errors");
        assert_eq!(
            build(&opts("  ", &d)).unwrap_err().code,
            ErrorCode::TaskRequired
        );
        assert_eq!(
            build(&opts("the and of", &d)).unwrap_err().code,
            ErrorCode::NoTerms
        );
        assert_eq!(
            build(&opts("x restart", &d.join("nope"))).unwrap_err().code,
            ErrorCode::BadRoot
        );
        let mut o = opts("restart", &d);
        o.now = Some("June 1".into());
        assert_eq!(build(&o).unwrap_err().code, ErrorCode::BadDate);
        assert_eq!(parse_pack("{}").unwrap_err().code, ErrorCode::BadPack);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn warning_needs_two_terms_or_a_tag() {
        let d = tmp("warn");
        write(
            &d,
            "memory/learnings/incidents/a.md",
            "# A\nThe gateway was slow.\n",
        );
        write(
            &d,
            "memory/learnings/incidents/b.md",
            "# B\nRestart the gateway by hand.\n",
        );
        write(
            &d,
            "notes/c.md",
            "---\ntags: [restart, anti-pattern]\n---\n# C\nNothing in common here.\n",
        );
        let p = build(&opts("restart the gateway", &d)).unwrap();
        let w: Vec<&str> = p.warnings.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(w, vec!["memory/learnings/incidents/b.md", "notes/c.md"]);
        assert_eq!(p.items.len(), 1);
        assert_eq!(p.items[0].kind, Kind::AntiPattern);
        assert_eq!(p.warnings[1].score.tag_match.matched_tags, vec!["restart"]);
        assert_eq!(p.warnings[1].score.lexical.score, 0.0);
        std::fs::remove_dir_all(&d).ok();
    }

    struct Fixed;
    impl SemanticScorer for Fixed {
        fn name(&self) -> &str {
            "fixed"
        }
        fn similarity(&self, _task: &str, text: &str) -> f64 {
            if text.to_lowercase().contains("reboot") {
                2.0
            } else {
                0.0
            }
        }
    }

    #[test]
    fn semantic_hook_adds_candidates_and_breakdown() {
        let d = tmp("semantic");
        write(&d, "a.md", "# Ops\nReboot the box when it hangs.\n");
        write(&d, "b.md", "# Gateway\nThe gateway listens on a port.\n");
        let o = opts("restart the gateway", &d);
        let lexical = build(&o).unwrap();
        assert_eq!(lexical.items.len(), 1);
        assert!(lexical.items[0].score.semantic.is_none());
        let p = build_with(&o, Some(&Fixed)).unwrap();
        assert_eq!(p.items.len(), 2);
        let a = p.items.iter().find(|i| i.path == "a.md").unwrap();
        let s = a.score.semantic.as_ref().unwrap();
        assert_eq!(
            (s.scorer.as_str(), s.similarity, s.score),
            ("fixed", 1.0, 2.0)
        );
        assert!(a.score.lexical.matched_terms.is_empty());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn verify_rejects_paths_outside_root() {
        assert!(safe_relative("a/b.md"));
        assert!(!safe_relative("../a.md"));
        assert!(!safe_relative("/etc/hosts"));
        assert!(!safe_relative("a/../../b"));
        assert!(!safe_relative(""));
    }
}
