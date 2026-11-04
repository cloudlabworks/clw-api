//! Item kinds and the rules that assign them.
//!
//! Order of precedence, first match wins: frontmatter `type:`, tag rules, path rules,
//! then an `Always` / `Never` / `Prefer` bullet is a rule; anything else is a fact.

use crate::scan::Frontmatter;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// A directive to follow ("Always run the tests before you push").
    Rule,
    /// A choice that was made, usually with its reason.
    Decision,
    /// Reference material; the default kind.
    Fact,
    /// A past mistake or incident: what not to do again.
    AntiPattern,
    /// A record of work done, such as a task ledger or receipt.
    Evidence,
}

impl Kind {
    pub const ALL: [Kind; 5] = [
        Kind::Rule,
        Kind::Decision,
        Kind::Fact,
        Kind::AntiPattern,
        Kind::Evidence,
    ];

    /// Parse a kind name or a common alias (`incident`, `ledger`, `adr`, ...).
    pub fn parse(s: &str) -> Option<Kind> {
        let s = s.trim().to_lowercase().replace('_', "-");
        Some(match s.as_str() {
            "rule" | "rules" | "directive" | "policy" => Kind::Rule,
            "decision" | "decisions" | "adr" => Kind::Decision,
            "fact" | "facts" | "reference" | "note" => Kind::Fact,
            "anti-pattern" | "antipattern" | "anti-patterns" | "incident" | "pitfall"
            | "mistake" => Kind::AntiPattern,
            "evidence" | "ledger" | "receipt" | "log" => Kind::Evidence,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Rule => "rule",
            Kind::Decision => "decision",
            Kind::Fact => "fact",
            Kind::AntiPattern => "anti-pattern",
            Kind::Evidence => "evidence",
        }
    }

    /// Multiplier applied to the item's relevance.
    pub fn weight(self) -> f64 {
        match self {
            Kind::AntiPattern => 1.3,
            Kind::Rule => 1.2,
            Kind::Decision => 1.1,
            Kind::Fact => 1.0,
            Kind::Evidence => 0.8,
        }
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Path and tag conventions that assign a kind. Rules are tried in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindRules {
    /// `(glob, kind)`: a root-relative path matching the glob gets the kind.
    pub paths: Vec<(String, Kind)>,
    /// `(tag, kind)`: a file whose frontmatter tags include the tag gets the kind.
    pub tags: Vec<(String, Kind)>,
}

impl Default for KindRules {
    fn default() -> Self {
        let p = |g: &str, k| (g.to_owned(), k);
        KindRules {
            paths: vec![
                p("memory/learnings/incidents/**", Kind::AntiPattern),
                p("memory/tasks/*.md", Kind::Evidence),
                p("**/decisions/**", Kind::Decision),
            ],
            tags: vec![
                p("anti-pattern", Kind::AntiPattern),
                p("antipattern", Kind::AntiPattern),
                p("incident", Kind::AntiPattern),
            ],
        }
    }
}

impl KindRules {
    pub fn empty() -> Self {
        KindRules {
            paths: Vec::new(),
            tags: Vec::new(),
        }
    }

    /// Kind of an item and the reason, e.g. `path:memory/tasks/*.md`.
    pub fn classify(&self, path: &str, fm: &Frontmatter, rule_bullet: bool) -> (Kind, String) {
        if let Some(k) = fm.doc_type.as_deref().and_then(Kind::parse) {
            return (k, "frontmatter:type".into());
        }
        for (tag, k) in &self.tags {
            if fm
                .tags
                .iter()
                .any(|t| normalize_tag(t) == normalize_tag(tag))
            {
                return (*k, format!("tag:{tag}"));
            }
        }
        for (glob, k) in &self.paths {
            if glob_match(glob, path) {
                return (*k, format!("path:{glob}"));
            }
        }
        if rule_bullet {
            return (Kind::Rule, "bullet:always-never-prefer".into());
        }
        (Kind::Fact, "default".into())
    }
}

fn normalize_tag(t: &str) -> String {
    t.trim()
        .trim_start_matches('#')
        .to_lowercase()
        .replace('_', "-")
}

/// Match a `/`-separated relative path against a glob: `**` is any number of whole
/// segments (including none), `*` any run of characters within a segment, `?` one character.
pub fn glob_match(pattern: &str, path: &str) -> bool {
    let p: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let s: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match_segments(&p, &s)
}

fn match_segments(p: &[&str], s: &[&str]) -> bool {
    match p.split_first() {
        None => s.is_empty(),
        Some((&"**", rest)) => (0..=s.len()).any(|i| match_segments(rest, &s[i..])),
        Some((seg, rest)) => {
            !s.is_empty()
                && match_segment(
                    &seg.chars().collect::<Vec<_>>(),
                    &s[0].chars().collect::<Vec<_>>(),
                )
                && match_segments(rest, &s[1..])
        }
    }
}

/// Wildcard match within one segment, with single-star backtracking.
fn match_segment(p: &[char], s: &[char]) -> bool {
    let (mut pi, mut si) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, si));
            pi += 1;
        } else if let Some((sp, ss)) = star {
            pi = sp + 1;
            si = ss + 1;
            star = Some((sp, ss + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        assert!(glob_match(
            "memory/learnings/incidents/**",
            "memory/learnings/incidents/a.md"
        ));
        assert!(glob_match(
            "memory/learnings/incidents/**",
            "memory/learnings/incidents/2026/b.md"
        ));
        assert!(!glob_match(
            "memory/learnings/incidents/**",
            "memory/learnings/other.md"
        ));
        assert!(glob_match("memory/tasks/*.md", "memory/tasks/x.md"));
        assert!(!glob_match("memory/tasks/*.md", "memory/tasks/sub/x.md"));
        assert!(!glob_match("memory/tasks/*.md", "memory/tasks/x.txt"));
        assert!(glob_match("**/decisions/**", "decisions/a.md"));
        assert!(glob_match("**/decisions/**", "a/b/decisions/c/d.md"));
        assert!(glob_match("USER.md", "USER.md"));
        assert!(glob_match("*.md", "USER.md"));
        assert!(!glob_match("*.md", "a/USER.md"));
        assert!(glob_match("**/*.md", "a/USER.md"));
        assert!(glob_match("a?c*.md", "abcdef.md"));
        assert!(glob_match("*a*b", "xxaxxbxxb"));
        assert!(!glob_match("*a*b", "xxaxxbxxc"));
        assert!(glob_match("logs/**", "logs"));
    }

    fn fm(doc_type: Option<&str>, tags: &[&str]) -> Frontmatter {
        Frontmatter {
            doc_type: doc_type.map(str::to_owned),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            ..Frontmatter::default()
        }
    }

    #[test]
    fn precedence_is_type_tag_path_bullet_default() {
        let r = KindRules::default();
        let inc = "memory/learnings/incidents/x.md";
        assert_eq!(
            r.classify(inc, &fm(Some("decision"), &["incident"]), true)
                .0,
            Kind::Decision
        );
        assert_eq!(
            r.classify("notes/a.md", &fm(None, &["Anti_Pattern"]), false),
            (Kind::AntiPattern, "tag:anti-pattern".into())
        );
        assert_eq!(
            r.classify(inc, &fm(None, &[]), true),
            (
                Kind::AntiPattern,
                "path:memory/learnings/incidents/**".into()
            )
        );
        assert_eq!(
            r.classify("memory/tasks/t.md", &fm(Some("whatever"), &[]), false)
                .0,
            Kind::Evidence
        );
        assert_eq!(r.classify("USER.md", &fm(None, &[]), true).0, Kind::Rule);
        assert_eq!(
            r.classify("USER.md", &fm(None, &[]), false),
            (Kind::Fact, "default".into())
        );
        assert_eq!(
            KindRules::empty()
                .classify(inc, &fm(None, &["incident"]), false)
                .0,
            Kind::Fact
        );
    }

    #[test]
    fn kind_names_and_aliases() {
        for k in Kind::ALL {
            assert_eq!(Kind::parse(k.as_str()), Some(k));
        }
        assert_eq!(Kind::parse("Incident"), Some(Kind::AntiPattern));
        assert_eq!(Kind::parse("anti_pattern"), Some(Kind::AntiPattern));
        assert_eq!(Kind::parse("ledger"), Some(Kind::Evidence));
        assert_eq!(Kind::parse("adr"), Some(Kind::Decision));
        assert_eq!(Kind::parse("banana"), None);
    }
}
