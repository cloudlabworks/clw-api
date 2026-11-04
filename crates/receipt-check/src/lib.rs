//! Deterministic checks of a completion claim against its evidence.
//!
//! A "receipt" is the evidence offered for a claim that some work is done. This crate runs
//! the checks that need no model: is there any evidence, does it contain a concrete artifact
//! (hash, URL, path, id, timestamp, command output, number), and does it mention the terms
//! the claim makes. It never calls a model and never touches the network.
//!
//! The semantics are ported from `deterministic_checks` in the Python done-claim checker
//! (bedrock-done-claim-checker, `src/handler.py`). That harness ran these checks before one
//! Bedrock call; here they stand alone, so the verdict is derived from the flags:
//!
//! | flags raised                          | verdict       |
//! |---------------------------------------|---------------|
//! | none                                  | `supported`   |
//! | `no_artifact` or `claim_terms_missing`| `partial`     |
//! | both of them                          | `unsupported` |
//! | `no_evidence`                         | `unsupported` |
//!
//! `supported` means "passed every deterministic check". It is a floor, not proof: a
//! person or a model still has to read the evidence to know the work is really done.

use regex::Regex;
use serde::Serialize;
use std::collections::BTreeSet;
use std::sync::LazyLock;

/// Longest accepted claim, in Unicode scalar values (same unit as Python `len(str)`).
pub const MAX_CLAIM: usize = 600;
/// Longest accepted evidence, in Unicode scalar values.
pub const MAX_EVIDENCE: usize = 4000;
/// Share of claim terms that may be missing from the evidence before it is flagged,
/// as a fraction `NUM / DEN` (0.6, compared exactly with integers).
const MISSING_NUM: usize = 3;
const MISSING_DEN: usize = 5;
/// How many missing terms the flag message lists.
const MESSAGE_TERMS: usize = 8;

/// Identifies this checker in every report.
pub const CHECKER: &str = concat!("receipt-check ", env!("CARGO_PKG_VERSION"));

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Supported,
    Partial,
    Unsupported,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FlagCode {
    /// Evidence is empty or whitespace only.
    NoEvidence,
    /// Evidence has no hash, URL, path, id, timestamp, command output or number.
    NoArtifact,
    /// More than 60% of the claim's terms never appear in the evidence.
    ClaimTermsMissing,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Flag {
    pub code: FlagCode,
    pub message: String,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub verdict: Verdict,
    pub flags: Vec<Flag>,
    /// Kinds of concrete artifact found in the evidence, in a fixed order.
    pub artifacts: Vec<&'static str>,
    /// Distinct claim terms (lowercased, stop words removed), sorted.
    pub claim_terms: Vec<String>,
    /// Claim terms whose stem never appears in the evidence, sorted.
    pub missing_terms: Vec<String>,
    /// Always false: this crate never calls a model.
    pub model_called: bool,
    pub checker: &'static str,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    ClaimRequired,
    TooLarge,
}

#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub claim: usize,
    pub evidence: usize,
}

pub const LIMITS: Limits = Limits {
    claim: MAX_CLAIM,
    evidence: MAX_EVIDENCE,
};

/// Bad input. Callers map `ClaimRequired` to HTTP 400 and `TooLarge` to HTTP 413.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct InputError {
    pub code: ErrorCode,
    pub error: String,
    pub limits: Limits,
}

impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.error)
    }
}

impl std::error::Error for InputError {}

static ARTIFACT_PATTERNS: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    let p = |name, rx: &str| (name, Regex::new(rx).expect("artifact pattern compiles"));
    vec![
        p("hash", r"\b[0-9a-f]{7,40}\b"),
        p("url", r"https?://\S+"),
        // Absolute or home-relative paths (/etc/hosts, ~/x, ./a/b), or relative paths
        // with a file extension (src/lib.rs).
        p(
            "path",
            r#"(?m)(?:^|[\s"'(=:\[])(?:~|\.{1,2})?/[A-Za-z0-9._-]+|\b[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)+\.[A-Za-z][A-Za-z0-9]{0,9}\b"#,
        ),
        // UUIDs, ARNs, cloud resource ids, issue/PR numbers, key=value ids.
        p(
            "id",
            r"(?m)\b[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}\b|\barn:[a-z0-9-]+:|\b(?:i|vpc|subnet|sg|ami|vol|eni|snap|rtb|igw|nat|tgw)-[0-9a-f]{8,17}\b|(?:^|[\s(])#\d+\b|\b(?:[A-Za-z]+[_-])?(?:id|ID)\s*[:=]\s*[A-Za-z0-9._:-]{3,}",
        ),
        p("timestamp", r"\b\d{4}-\d{2}-\d{2}(?:[ T]\d{2}:\d{2})?"),
        p(
            "command",
            r"(?m)^\$ |\bexit (?:code )?\d+\b|\bHTTP/\d(?:\.\d)? \d{3}\b|\b(?:PASS|FAIL|OK)\b",
        ),
        p("number", r"\b\d+(?:\.\d+)?\b"),
    ]
});

static CLAIM_WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[a-z][a-z0-9_-]{2,}").expect("claim word pattern compiles"));

const STOP: &[&str] = &[
    "the",
    "a",
    "an",
    "and",
    "or",
    "to",
    "of",
    "in",
    "on",
    "for",
    "is",
    "was",
    "are",
    "be",
    "been",
    "it",
    "this",
    "that",
    "with",
    "as",
    "by",
    "at",
    "from",
    "i",
    "we",
    "done",
    "complete",
    "completed",
    "finished",
    "successfully",
    "all",
    "now",
    "has",
    "have",
];

/// Crude suffix strip so "deployed" matches "deploy" and "tests" matches "pytest".
fn stem(word: &str) -> &str {
    for suf in ["ing", "ed", "es", "s"] {
        if word.ends_with(suf) && word.len() - suf.len() >= 4 {
            return &word[..word.len() - suf.len()];
        }
    }
    word
}

/// Kinds of concrete artifact present in `evidence`.
pub fn artifacts(evidence: &str) -> Vec<&'static str> {
    ARTIFACT_PATTERNS
        .iter()
        .filter(|(_, rx)| rx.is_match(evidence))
        .map(|(name, _)| *name)
        .collect()
}

/// Distinct, sorted claim terms with stop words removed.
pub fn claim_terms(claim: &str) -> Vec<String> {
    let lower = claim.to_lowercase();
    CLAIM_WORD
        .find_iter(&lower)
        .map(|m| m.as_str())
        .filter(|w| !STOP.contains(w))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn verdict_for(flags: &[Flag]) -> Verdict {
    if flags.iter().any(|f| f.code == FlagCode::NoEvidence) {
        return Verdict::Unsupported;
    }
    match flags.len() {
        0 => Verdict::Supported,
        1 => Verdict::Partial,
        _ => Verdict::Unsupported,
    }
}

/// Check `claim` against `evidence`.
///
/// The claim is trimmed; an empty claim is an error. Sizes are counted in Unicode scalar
/// values after trimming the claim (evidence is measured as given).
pub fn check(claim: &str, evidence: &str) -> Result<Report, InputError> {
    let claim = claim.trim();
    if claim.is_empty() {
        return Err(InputError {
            code: ErrorCode::ClaimRequired,
            error: "claim is required".into(),
            limits: LIMITS,
        });
    }
    if claim.chars().count() > MAX_CLAIM || evidence.chars().count() > MAX_EVIDENCE {
        return Err(InputError {
            code: ErrorCode::TooLarge,
            error: format!("limits: claim {MAX_CLAIM} chars, evidence {MAX_EVIDENCE} chars"),
            limits: LIMITS,
        });
    }

    let terms = claim_terms(claim);
    let mut flags = Vec::new();

    if evidence.trim().is_empty() {
        flags.push(Flag {
            code: FlagCode::NoEvidence,
            message: "no evidence given".into(),
        });
        return Ok(Report {
            verdict: verdict_for(&flags),
            flags,
            artifacts: Vec::new(),
            missing_terms: terms.clone(),
            claim_terms: terms,
            model_called: false,
            checker: CHECKER,
        });
    }

    let found = artifacts(evidence);
    if found.is_empty() {
        flags.push(Flag {
            code: FlagCode::NoArtifact,
            message: "evidence contains no concrete artifact (hash, URL, path, id, timestamp, command output, number)".into(),
        });
    }

    let ev = evidence.to_lowercase();
    let missing: Vec<String> = terms
        .iter()
        .filter(|w| !ev.contains(stem(w)))
        .cloned()
        .collect();
    if !terms.is_empty() && missing.len() * MISSING_DEN > terms.len() * MISSING_NUM {
        let shown: Vec<&str> = missing
            .iter()
            .take(MESSAGE_TERMS)
            .map(String::as_str)
            .collect();
        flags.push(Flag {
            code: FlagCode::ClaimTermsMissing,
            message: format!(
                "most claim terms never appear in the evidence: {}",
                shown.join(", ")
            ),
        });
    }

    Ok(Report {
        verdict: verdict_for(&flags),
        flags,
        artifacts: found,
        claim_terms: terms,
        missing_terms: missing,
        model_called: false,
        checker: CHECKER,
    })
}

/// [`check`] serialized to compact JSON: the report, or `{"code", "error", "limits"}`.
pub fn check_json(claim: &str, evidence: &str) -> String {
    let out = match check(claim, evidence) {
        Ok(report) => serde_json::to_string(&report),
        Err(err) => serde_json::to_string(&err),
    };
    out.expect("report serializes")
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use wasm_bindgen::prelude::*;

    /// `check(claim, evidence)` returning the JSON string from [`crate::check_json`].
    #[wasm_bindgen(js_name = check)]
    pub fn check_wasm(claim: &str, evidence: &str) -> String {
        crate::check_json(claim, evidence)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn codes(r: &Report) -> Vec<FlagCode> {
        r.flags.iter().map(|f| f.code).collect()
    }

    #[test]
    fn supported_when_no_flags() {
        let r = check(
            "Deployed the worker to staging",
            "$ wrangler deploy --env staging\nUploaded worker (1.2 sec)\nDeployed to https://staging.example.dev",
        )
        .unwrap();
        assert_eq!(r.verdict, Verdict::Supported);
        assert!(r.flags.is_empty());
        assert!(r.artifacts.contains(&"url"));
        assert!(r.artifacts.contains(&"command"));
        assert!(!r.model_called);
    }

    #[test]
    fn no_evidence_is_unsupported() {
        for ev in ["", "   \n\t "] {
            let r = check("tests pass", ev).unwrap();
            assert_eq!(r.verdict, Verdict::Unsupported);
            assert_eq!(codes(&r), vec![FlagCode::NoEvidence]);
            assert_eq!(r.flags[0].message, "no evidence given");
            assert!(r.artifacts.is_empty());
            assert_eq!(r.missing_terms, r.claim_terms);
        }
    }

    #[test]
    fn no_artifact_alone_is_partial() {
        let r = check("updated the readme", "I updated the readme file for you").unwrap();
        assert_eq!(codes(&r), vec![FlagCode::NoArtifact]);
        assert_eq!(r.verdict, Verdict::Partial);
        assert!(r.artifacts.is_empty());
    }

    #[test]
    fn missing_terms_alone_is_partial() {
        let r = check(
            "migrated database schema and rotated credentials",
            "commit 3f9a2b1c pushed",
        )
        .unwrap();
        assert_eq!(codes(&r), vec![FlagCode::ClaimTermsMissing]);
        assert_eq!(r.verdict, Verdict::Partial);
        assert!(r.flags[0]
            .message
            .starts_with("most claim terms never appear in the evidence: "));
        assert_eq!(
            r.missing_terms,
            vec!["credentials", "database", "migrated", "rotated", "schema"]
        );
    }

    #[test]
    fn both_soft_flags_are_unsupported() {
        let r = check("rotated credentials everywhere", "trust me, it went fine").unwrap();
        assert_eq!(
            codes(&r),
            vec![FlagCode::NoArtifact, FlagCode::ClaimTermsMissing]
        );
        assert_eq!(r.verdict, Verdict::Unsupported);
    }

    #[test]
    fn missing_ratio_threshold_is_strictly_above_sixty_percent() {
        // 5 terms, 3 missing = exactly 0.6: not flagged (Python `> 0.6`).
        let r = check("alpha bravo charlie delta echo", "alpha bravo 42").unwrap();
        assert_eq!(r.claim_terms.len(), 5);
        assert_eq!(r.missing_terms.len(), 3);
        assert!(r.flags.is_empty());
        // 4 missing of 5: flagged.
        let r = check("alpha bravo charlie delta echo", "alpha 42").unwrap();
        assert_eq!(codes(&r), vec![FlagCode::ClaimTermsMissing]);
    }

    #[test]
    fn message_lists_at_most_eight_terms() {
        let claim = "alpha bravo charlie delta echo foxtrot golf hotel india juliet";
        let r = check(claim, "exit 0").unwrap();
        assert_eq!(r.missing_terms.len(), 10);
        let listed = r.flags[0].message.split(": ").nth(1).unwrap();
        assert_eq!(listed.split(", ").count(), 8);
    }

    #[test]
    fn stems_match_inflected_claim_terms() {
        assert_eq!(stem("deployed"), "deploy");
        assert_eq!(stem("tests"), "test");
        assert_eq!(stem("running"), "runn");
        assert_eq!(stem("uses"), "uses"); // stem would be shorter than 4
        let r = check("tests deployed", "pytest: 12 passed; deploy ok").unwrap();
        assert!(r.missing_terms.is_empty());
    }

    #[test]
    fn stop_words_and_short_words_are_ignored() {
        assert!(claim_terms("It is done and all complete, ok").is_empty());
        assert_eq!(claim_terms("Fixed THE bug in CI"), vec!["bug", "fixed"]);
        // A claim with no terms cannot raise the missing-terms flag.
        let r = check("done", "exit 0").unwrap();
        assert_eq!(r.verdict, Verdict::Supported);
    }

    #[test]
    fn each_artifact_kind_is_detected() {
        let cases = [
            ("hash", "at commit 3f9a2b1"),
            ("url", "see https://example.com/x"),
            ("path", "wrote /etc/hosts"),
            ("path", "edited src/lib.rs"),
            ("path", "saved to ~/notes"),
            ("id", "request 123e4567-e89b-12d3-a456-426614174000"),
            ("id", "role arn:aws:iam::"),
            ("id", "started i-0abc12345def67890"),
            ("id", "merged #42"),
            ("id", "run_id=abc_xyz"),
            ("timestamp", "on 2026-11-03 14:02"),
            ("command", "$ make test"),
            ("command", "process exit code 0"),
            ("command", "HTTP/1.1 200"),
            ("command", "result: PASS"),
            ("number", "took 12 seconds"),
        ];
        for (kind, ev) in cases {
            assert!(artifacts(ev).contains(&kind), "{kind} not found in {ev:?}");
        }
        assert!(artifacts("nothing concrete here").is_empty());
        assert!(!artifacts("this and/or that").contains(&"path"));
        assert!(!artifacts("they paid: yes").contains(&"id"));
        assert!(!artifacts("HTTP/1.1 200").contains(&"path"));
    }

    #[test]
    fn artifacts_are_reported_in_fixed_order() {
        let found = artifacts("2026-11-03 https://x.dev/a 3f9a2b1 exit 0");
        let order = [
            "hash",
            "url",
            "path",
            "id",
            "timestamp",
            "command",
            "number",
        ];
        let positions: Vec<usize> = found
            .iter()
            .map(|k| order.iter().position(|o| o == k).unwrap())
            .collect();
        assert!(positions.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn empty_claim_is_an_error() {
        for claim in ["", "  \n "] {
            let e = check(claim, "exit 0").unwrap_err();
            assert_eq!(e.code, ErrorCode::ClaimRequired);
            assert_eq!(e.error, "claim is required");
        }
    }

    #[test]
    fn size_limits_count_characters() {
        let at_limit = "é".repeat(MAX_CLAIM);
        assert!(check(&at_limit, "exit 0").is_ok());
        let e = check(&"é".repeat(MAX_CLAIM + 1), "exit 0").unwrap_err();
        assert_eq!(e.code, ErrorCode::TooLarge);
        assert_eq!(e.error, "limits: claim 600 chars, evidence 4000 chars");
        assert!(check("ok claim", &"x".repeat(MAX_EVIDENCE)).is_ok());
        let e = check("ok claim", &"x".repeat(MAX_EVIDENCE + 1)).unwrap_err();
        assert_eq!(e.code, ErrorCode::TooLarge);
        // The claim is trimmed before measuring.
        let padded = format!("  {}  ", "a".repeat(MAX_CLAIM));
        assert!(check(&padded, "exit 0").is_ok());
    }

    #[test]
    fn json_shapes() {
        let ok: serde_json::Value =
            serde_json::from_str(&check_json("updated the readme", "the readme was updated"))
                .unwrap();
        assert_eq!(ok["verdict"], "partial");
        assert_eq!(ok["flags"][0]["code"], "no_artifact");
        assert_eq!(ok["model_called"], false);
        let err: serde_json::Value = serde_json::from_str(&check_json("", "x")).unwrap();
        assert_eq!(err["code"], "claim_required");
        assert_eq!(err["limits"]["claim"], 600);
        assert_eq!(err["limits"]["evidence"], 4000);
        assert!(err.get("verdict").is_none());
    }
}
