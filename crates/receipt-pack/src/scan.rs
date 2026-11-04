//! Finding files, reading frontmatter and splitting files into items.

use crate::classify::glob_match;
use crate::date::parse_date;
use std::path::Path;

/// Directories never walked, besides any whose name starts with a dot.
const SKIP_DIRS: &[&str] = &["node_modules", "target"];
/// Frontmatter must close within this many lines or it is treated as body text.
const FRONTMATTER_MAX: usize = 200;

/// The frontmatter fields this crate reads. Everything else is ignored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frontmatter {
    /// `type:` (or `kind:`), lowercased.
    pub doc_type: Option<String>,
    /// `tags:` as an inline list (`[a, b]` or `a, b`) or a block list, lowercased.
    pub tags: Vec<String>,
    /// The first of `updated`, `last_updated`, `date`, `created` that parses, with its key.
    pub date: Option<(String, i64)>,
    /// 0-based index of the first body line.
    pub body_start: usize,
}

/// Lines of a file without their terminators (`\n`, and a `\r` before it).
pub fn split_lines(content: &str) -> Vec<&str> {
    content
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect()
}

fn unquote(v: &str) -> &str {
    let v = v.trim();
    for q in ['"', '\''] {
        if let Some(inner) = v.strip_prefix(q).and_then(|v| v.strip_suffix(q)) {
            return inner;
        }
    }
    v
}

fn push_tags(out: &mut Vec<String>, v: &str) {
    let v = v.trim();
    let v = v
        .strip_prefix('[')
        .and_then(|v| v.strip_suffix(']'))
        .unwrap_or(v);
    for t in v.split([',', ' ']) {
        let t = unquote(t).trim_start_matches('#').to_lowercase();
        if !t.is_empty() && !out.contains(&t) {
            out.push(t);
        }
    }
}

const DATE_KEYS: &[&str] = &["updated", "last_updated", "last-updated", "date", "created"];

pub fn parse_frontmatter(lines: &[&str]) -> Frontmatter {
    let mut fm = Frontmatter::default();
    if lines.first().map(|l| l.trim_end()) != Some("---") {
        return fm;
    }
    let Some(close) = lines
        .iter()
        .skip(1)
        .take(FRONTMATTER_MAX)
        .position(|l| matches!(l.trim_end(), "---" | "..."))
        .map(|i| i + 1)
    else {
        return fm;
    };
    let mut in_tags = false;
    let mut best_date: Option<(usize, String, i64)> = None;
    for line in &lines[1..close] {
        let t = line.trim();
        if let Some(item) = t.strip_prefix("- ") {
            if in_tags {
                push_tags(&mut fm.tags, item);
            }
            continue;
        }
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            continue;
        };
        let key = k.trim().to_lowercase();
        let v = unquote(v);
        in_tags = false;
        match key.as_str() {
            "type" | "kind" if !v.is_empty() => fm.doc_type = Some(v.to_lowercase()),
            "tags" | "tag" => {
                if v.is_empty() {
                    in_tags = true;
                } else {
                    push_tags(&mut fm.tags, v);
                }
            }
            _ => {
                if let Some(prio) = DATE_KEYS.iter().position(|d| *d == key) {
                    if let Some(days) = parse_date(v) {
                        if best_date.as_ref().map_or(true, |(p, _, _)| prio < *p) {
                            best_date = Some((prio, key.clone(), days));
                        }
                    }
                }
            }
        }
    }
    fm.date = best_date.map(|(_, k, d)| (k, d));
    fm.body_start = close + 1;
    fm
}

/// A `YYYY-MM-DD` anywhere in the file name, e.g. `2026-03-02-token-in-url.md`.
pub fn filename_date(rel: &str) -> Option<i64> {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    (0..name.len().saturating_sub(9))
        .filter_map(|i| name.get(i..i + 10))
        .find_map(parse_date)
}

/// One item before scoring: a 1-based inclusive line range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    pub start: usize,
    pub end: usize,
    /// Text of the nearest heading above (or at) the first line.
    pub heading: Option<String>,
    /// The item is a single `Always` / `Never` / `Prefer` bullet.
    pub rule_bullet: bool,
}

fn heading_text(line: &str) -> Option<String> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let t = &line[indent..];
    let hashes = t.len() - t.trim_start_matches('#').len();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &t[hashes..];
    if !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
        return None;
    }
    Some(rest.trim().trim_end_matches('#').trim().to_owned())
}

fn is_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// A Markdown bullet whose text starts with `Always`, `Never` or `Prefer`, optionally bold.
pub fn is_rule_bullet(line: &str) -> bool {
    let t = line.trim_start();
    let Some(rest) = ["- ", "* ", "+ "].iter().find_map(|b| t.strip_prefix(b)) else {
        return false;
    };
    let rest = rest.trim_start().trim_start_matches(['*', '_']);
    ["Always", "Never", "Prefer"].iter().any(|w| {
        rest.strip_prefix(w)
            .is_some_and(|after| after.chars().next().map_or(true, |c| !c.is_alphanumeric()))
    })
}

/// Split the body of a file into items: one per section (a heading and what follows it),
/// sections longer than `max_lines` cut into pieces at blank lines, and every rule bullet
/// (with its indented continuation lines) as an item of its own. Leading and trailing blank
/// lines are trimmed; pieces with nothing but headings are dropped.
pub fn chunk(lines: &[&str], body_start: usize, max_lines: usize) -> Vec<Chunk> {
    let max_lines = max_lines.max(1);
    let mut fenced = vec![false; lines.len()];
    let mut heading_at: Vec<Option<String>> = vec![None; lines.len()];
    let mut in_fence = false;
    for i in body_start..lines.len() {
        if is_fence(lines[i]) {
            fenced[i] = true;
            in_fence = !in_fence;
        } else if in_fence {
            fenced[i] = true;
        } else {
            heading_at[i] = heading_text(lines[i]);
        }
    }

    let ctx = Ctx {
        lines,
        fenced: &fenced,
        heading_at: &heading_at,
        max_lines,
    };
    let mut out = Vec::new();
    let mut heading: Option<String> = None;
    let mut start = body_start;
    for (i, h) in heading_at.iter().enumerate().skip(body_start) {
        if let Some(h) = h {
            ctx.section(start, i, &heading, &mut out);
            heading = Some(h.clone());
            start = i;
        }
    }
    ctx.section(start, lines.len(), &heading, &mut out);
    out
}

struct Ctx<'a> {
    lines: &'a [&'a str],
    fenced: &'a [bool],
    heading_at: &'a [Option<String>],
    max_lines: usize,
}

impl Ctx<'_> {
    fn blank(&self, i: usize) -> bool {
        self.lines[i].trim().is_empty()
    }

    fn section(&self, a: usize, b: usize, heading: &Option<String>, out: &mut Vec<Chunk>) {
        let heading = if a < b {
            self.heading_at[a].clone().or(heading.clone())
        } else {
            None
        };
        let (mut i, mut run) = (a, a);
        while i < b {
            if !self.fenced[i] && is_rule_bullet(self.lines[i]) {
                self.run(run, i, &heading, out);
                let indent = indent_of(self.lines[i]);
                let mut j = i + 1;
                while j < b && !self.blank(j) && indent_of(self.lines[j]) > indent {
                    j += 1;
                }
                out.push(Chunk {
                    start: i + 1,
                    end: j,
                    heading: heading.clone(),
                    rule_bullet: true,
                });
                i = j;
                run = j;
            } else {
                i += 1;
            }
        }
        self.run(run, b, &heading, out);
    }

    fn run(&self, mut a: usize, mut b: usize, heading: &Option<String>, out: &mut Vec<Chunk>) {
        while a < b && self.blank(a) {
            a += 1;
        }
        while b > a && self.blank(b - 1) {
            b -= 1;
        }
        while a < b {
            let mut end = b;
            if b - a > self.max_lines {
                end = a + self.max_lines;
                // Prefer to cut after a blank line in the second half of the window.
                if let Some(cut) = (a + self.max_lines / 2..end).rev().find(|&i| self.blank(i)) {
                    end = cut;
                }
            }
            let mut e = end;
            while e > a && self.blank(e - 1) {
                e -= 1;
            }
            let substantive =
                (a..e).any(|i| !self.blank(i) && (self.fenced[i] || self.heading_at[i].is_none()));
            if substantive {
                out.push(Chunk {
                    start: a + 1,
                    end: e,
                    heading: heading.clone(),
                    rule_bullet: false,
                });
            }
            a = end;
            while a < b && self.blank(a) {
                a += 1;
            }
        }
    }
}

/// Root-relative paths of the files to index, sorted, with `/` separators. Skips symlinks,
/// dot-directories and dot-files, `node_modules`, `target`, paths matching an `exclude`
/// glob, files without one of `exts`, and files over `max_bytes`.
pub fn walk(root: &Path, exclude: &[String], exts: &[String], max_bytes: u64) -> Vec<String> {
    let mut out = Vec::new();
    walk_dir(root, "", exclude, exts, max_bytes, &mut out);
    out
}

fn walk_dir(
    dir: &Path,
    rel: &str,
    exclude: &[String],
    exts: &[String],
    max_bytes: u64,
    out: &mut Vec<String>,
) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let child = if rel.is_empty() {
            name.to_owned()
        } else {
            format!("{rel}/{name}")
        };
        if exclude.iter().any(|g| glob_match(g, &child)) {
            continue;
        }
        let Ok(ft) = e.file_type() else {
            continue;
        };
        if ft.is_dir() {
            if !SKIP_DIRS.contains(&name) {
                walk_dir(&e.path(), &child, exclude, exts, max_bytes, out);
            }
        } else if ft.is_file() {
            let ext_ok = name
                .rsplit_once('.')
                .is_some_and(|(_, x)| exts.iter().any(|w| w.eq_ignore_ascii_case(x)));
            let size_ok = e.metadata().is_ok_and(|m| m.len() <= max_bytes);
            if ext_ok && size_ok {
                out.push(child);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_fields() {
        let src = "---\ntype: Incident\ntags: [git, \"Anti-Pattern\"]\ncreated: 2026-01-01\nupdated: '2026-03-04'\ntitle: x: y\n---\n# Body\ntext";
        let lines = split_lines(src);
        let fm = parse_frontmatter(&lines);
        assert_eq!(fm.doc_type.as_deref(), Some("incident"));
        assert_eq!(fm.tags, vec!["git", "anti-pattern"]);
        assert_eq!(
            fm.date,
            Some(("updated".into(), parse_date("2026-03-04").unwrap()))
        );
        assert_eq!(fm.body_start, 7);
        assert_eq!(lines[fm.body_start], "# Body");
    }

    #[test]
    fn frontmatter_block_tags_and_absence() {
        let lines = split_lines("---\ntags:\n  - one\n  - two\ndate: bad\n---\nx");
        let fm = parse_frontmatter(&lines);
        assert_eq!(fm.tags, vec!["one", "two"]);
        assert_eq!(fm.date, None);
        let none = parse_frontmatter(&split_lines("# Title\n---\n"));
        assert_eq!(none, Frontmatter::default());
        // Never closed: body text, not frontmatter.
        let open = parse_frontmatter(&split_lines("---\ntype: rule\n"));
        assert_eq!(open.body_start, 0);
        assert_eq!(open.doc_type, None);
    }

    #[test]
    fn filename_dates() {
        assert_eq!(
            filename_date("memory/2026-11-03.md"),
            parse_date("2026-11-03")
        );
        assert_eq!(
            filename_date("a/2026-03-02-token.md"),
            parse_date("2026-03-02")
        );
        assert_eq!(filename_date("a/2026-13-02.md"), None);
        assert_eq!(filename_date("USER.md"), None);
    }

    #[test]
    fn rule_bullets() {
        assert!(is_rule_bullet("- **Always** run the tests."));
        assert!(is_rule_bullet("  * Never: push to main"));
        assert!(is_rule_bullet("+ Prefer SSH"));
        assert!(!is_rule_bullet("- Preferences are listed below"));
        assert!(!is_rule_bullet("Always run the tests"));
        assert!(!is_rule_bullet("- always lowercase"));
    }

    fn ranges(src: &str, max: usize) -> Vec<(usize, usize, bool, Option<String>)> {
        let lines = split_lines(src);
        let fm = parse_frontmatter(&lines);
        chunk(&lines, fm.body_start, max)
            .into_iter()
            .map(|c| (c.start, c.end, c.rule_bullet, c.heading))
            .collect()
    }

    #[test]
    fn chunks_by_heading_and_rule_bullet() {
        let src = "\
---
type: fact
---
# Directives

- **Always** run tests.
  Even for docs changes.
- Never push to main.
- Short subjects.

## Notes
Some text.
```
# not a heading
- Never inside a fence
```
";
        let h = |s: &str| Some(s.to_owned());
        assert_eq!(
            ranges(src, 40),
            vec![
                (6, 7, true, h("Directives")),
                (8, 8, true, h("Directives")),
                (9, 9, false, h("Directives")),
                (11, 16, false, h("Notes")),
            ]
        );
    }

    #[test]
    fn long_sections_split_at_blank_lines() {
        let mut src = String::from("# Log\n");
        for i in 0..30 {
            src.push_str(&format!("entry {i}\n"));
            if i % 5 == 4 {
                src.push('\n');
            }
        }
        let r = ranges(&src, 10);
        assert!(r.iter().all(|(s, e, _, _)| e - s < 10));
        assert_eq!(r[0].0, 1);
        assert_eq!(r.last().unwrap().1, 36);
        // Pieces never start or end on a blank line.
        let lines = split_lines(&src);
        for (s, e, _, _) in &r {
            assert!(!lines[s - 1].trim().is_empty() && !lines[e - 1].trim().is_empty());
        }
    }

    #[test]
    fn heading_only_sections_are_dropped() {
        assert_eq!(
            ranges("# A\n\n## B\ntext\n", 40),
            vec![(3, 4, false, Some("B".into()))]
        );
        assert!(ranges("", 40).is_empty());
    }
}
