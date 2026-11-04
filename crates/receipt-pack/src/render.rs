//! Markdown rendering of a pack, for pasting into a prompt or reading.

use crate::{Item, Pack};
use std::fmt::Write;

macro_rules! emit {
    ($out:expr, $($arg:tt)*) => {
        let _ = writeln!($out, $($arg)*);
    };
}

/// A code fence longer than any run of backticks in `text`.
fn fence(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    "`".repeat((longest + 1).max(3))
}

fn item(out: &mut String, label: &str, it: &Item) {
    let s = &it.score;
    emit!(
        out,
        "\n### {label}. {}: `{}` lines {}-{}\n",
        it.kind,
        it.path,
        it.lines.start,
        it.lines.end
    );
    if let Some(h) = &it.heading {
        emit!(out, "- section: {h}");
    }
    let semantic = s
        .semantic
        .as_ref()
        .map(|x| format!(" + semantic {}", x.score))
        .unwrap_or_default();
    emit!(
        out,
        "- score: {} = (lexical {} + tag {}{semantic}) x kind {} x recency {}",
        s.total,
        s.lexical.score,
        s.tag_match.score,
        s.kind_weight,
        s.recency.factor
    );
    let terms = if s.lexical.matched_terms.is_empty() {
        "none".to_owned()
    } else {
        s.lexical.matched_terms.join(", ")
    };
    let tags = if s.tag_match.matched_tags.is_empty() {
        String::new()
    } else {
        format!("; matched tags: {}", s.tag_match.matched_tags.join(", "))
    };
    emit!(out, "- matched terms: {terms}{tags}");
    let age = s
        .recency
        .age_days
        .map(|d| format!(", {d} days old"))
        .unwrap_or_default();
    emit!(
        out,
        "- recency: {} ({}{age}); kind from {}",
        s.recency.date.as_deref().unwrap_or("unknown"),
        s.recency.source,
        it.kind_reason
    );
    let git = match (&it.git_commit, it.git_modified) {
        (Some(c), Some(true)) => format!(", commit `{}` (modified since)", short(c)),
        (Some(c), _) => format!(", commit `{}`", short(c)),
        (None, Some(_)) => ", not committed".to_owned(),
        (None, None) => String::new(),
    };
    emit!(
        out,
        "- evidence: sha256 `{}`{git}, {} tokens",
        it.sha256,
        it.tokens
    );
    let f = fence(&it.excerpt);
    emit!(out, "\n{f}\n{}\n{f}", it.excerpt);
}

fn short(sha: &str) -> &str {
    sha.get(..12).unwrap_or(sha)
}

/// The pack as Markdown: a header, the WARNING section (when any anti-pattern matched),
/// then the items in score order. Use the JSON form for `receipt-pack verify`.
pub fn markdown(pack: &Pack) -> String {
    let mut out = String::new();
    let b = &pack.budget;
    let c = &pack.counts;
    emit!(out, "# Context pack\n");
    emit!(out, "- task: {}", pack.task);
    emit!(out, "- terms: {}", pack.terms.join(", "));
    emit!(out, "- root: `{}`", pack.root);
    let over = if b.over_budget {
        " (over budget: warnings are never cut)"
    } else {
        ""
    };
    emit!(
        out,
        "- budget: {} of {} tokens used, estimated as characters / 4{over}",
        b.used,
        b.tokens
    );
    emit!(
        out,
        "- items: {} warnings, {} included, {} dropped for budget; {} candidates of {} items in {} files",
        c.warnings,
        c.included,
        c.dropped_for_budget,
        c.candidates,
        c.items,
        c.files
    );
    let head = pack
        .git_head
        .as_deref()
        .map(|h| format!(", git HEAD `{}`", short(h)))
        .unwrap_or_default();
    emit!(
        out,
        "- generated: {} by {}{head}",
        pack.generated,
        pack.tool
    );

    if !pack.warnings.is_empty() {
        emit!(out, "\n## WARNING: anti-patterns that match this task\n");
        emit!(
            out,
            "These are recorded mistakes that look like this task. Read them before you start."
        );
        for (i, it) in pack.warnings.iter().enumerate() {
            item(&mut out, &format!("W{}", i + 1), it);
        }
    }

    emit!(out, "\n## Items");
    if pack.items.is_empty() {
        emit!(out, "\nNo other items matched within the budget.");
    }
    for (i, it) in pack.items.iter().enumerate() {
        item(&mut out, &(i + 1).to_string(), it);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::fence;

    #[test]
    fn fence_outgrows_backticks_in_text() {
        assert_eq!(fence("plain"), "```");
        assert_eq!(fence("a ``` b"), "````");
        assert_eq!(fence("`````"), "``````");
    }
}
