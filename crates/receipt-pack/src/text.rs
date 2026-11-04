//! Terms and lexical relevance: tokenizer, crude stemmer and BM25.

use std::collections::HashMap;

/// BM25 term-frequency saturation.
pub const K1: f64 = 1.2;
/// BM25 length normalization.
pub const B: f64 = 0.75;

const STOP: &[&str] = &[
    "a", "about", "after", "all", "also", "an", "and", "any", "are", "as", "at", "be", "been",
    "but", "by", "can", "could", "do", "does", "for", "from", "has", "have", "how", "if", "in",
    "into", "is", "it", "its", "just", "me", "my", "no", "not", "of", "on", "or", "our", "out",
    "over", "should", "so", "some", "than", "that", "the", "their", "them", "then", "there",
    "these", "this", "those", "to", "up", "via", "was", "we", "were", "what", "when", "where",
    "which", "who", "why", "will", "with", "would", "you", "your",
];

/// A task term: the word as written (lowercased) and the stem it is matched by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Term {
    pub word: String,
    pub stem: String,
}

/// Lowercased alphanumeric runs of `text`.
fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
}

fn keep(word: &str) -> bool {
    word.chars().count() >= 2 && !STOP.contains(&word) && !word.chars().all(|c| c.is_ascii_digit())
}

/// Crude English suffix strip so "pushes", "pushed" and "pushing" all become "push", and
/// "issue" and "issues" both become "issu". Non-ASCII words are returned unchanged.
pub fn stem(word: &str) -> String {
    if !word.is_ascii() {
        return word.to_owned();
    }
    let mut w = word;
    let mut verb = false;
    if w.len() >= 6 && w.ends_with("ing") {
        w = &w[..w.len() - 3];
        verb = true;
    } else if w.len() >= 5 && w.ends_with("ed") {
        w = &w[..w.len() - 2];
        verb = true;
    } else if w.len() >= 5
        && w.ends_with("es")
        && ["s", "x", "z", "ch", "sh"]
            .iter()
            .any(|e| w[..w.len() - 2].ends_with(e))
    {
        w = &w[..w.len() - 2];
    } else if w.len() >= 4 && w.ends_with('s') && !w.ends_with("ss") {
        w = &w[..w.len() - 1];
    }
    let b = w.as_bytes();
    // "committed" -> "committ" -> "commit"; keep "ll", "ss", "zz" ("installed", "passed").
    if verb
        && b.len() >= 4
        && b[b.len() - 1] == b[b.len() - 2]
        && !b"aeioulsz".contains(&b[b.len() - 1])
    {
        w = &w[..w.len() - 1];
    }
    if w.len() >= 5 && w.ends_with('e') {
        w = &w[..w.len() - 1];
    }
    w.to_owned()
}

/// Stemmed terms of `text`, in order, with repeats (a document for BM25).
pub fn terms(text: &str) -> Vec<String> {
    words(text).filter(|w| keep(w)).map(|w| stem(&w)).collect()
}

/// Distinct task terms in order of first appearance, one per stem.
pub fn task_terms(task: &str) -> Vec<Term> {
    let mut out: Vec<Term> = Vec::new();
    for word in words(task).filter(|w| keep(w)) {
        let stem = stem(&word);
        if !out.iter().any(|t| t.stem == stem) {
            out.push(Term { word, stem });
        }
    }
    out
}

/// BM25 score of every document against the query stems, with the indexes of the query
/// stems each document contains. `idf = ln(1 + (N - df + 0.5) / (df + 0.5))`.
pub fn bm25(query: &[String], docs: &[Vec<String>]) -> Vec<(f64, Vec<usize>)> {
    let n = docs.len() as f64;
    let avgdl = (docs.iter().map(Vec::len).sum::<usize>() as f64 / n.max(1.0)).max(1.0);
    let tfs: Vec<HashMap<&str, usize>> = docs
        .iter()
        .map(|d| {
            let mut m = HashMap::new();
            for t in d {
                *m.entry(t.as_str()).or_insert(0) += 1;
            }
            m
        })
        .collect();
    let idf: Vec<f64> = query
        .iter()
        .map(|q| {
            let df = tfs.iter().filter(|m| m.contains_key(q.as_str())).count() as f64;
            (1.0 + (n - df + 0.5) / (df + 0.5)).ln()
        })
        .collect();
    tfs.iter()
        .zip(docs)
        .map(|(tf, doc)| {
            let dl = doc.len() as f64;
            let mut score = 0.0;
            let mut matched = Vec::new();
            for (i, q) in query.iter().enumerate() {
                if let Some(&f) = tf.get(q.as_str()) {
                    let f = f as f64;
                    score += idf[i] * f * (K1 + 1.0) / (f + K1 * (1.0 - B + B * dl / avgdl));
                    matched.push(i);
                }
            }
            (score, matched)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_group_inflections() {
        for (w, s) in [
            ("push", "push"),
            ("pushes", "push"),
            ("pushed", "push"),
            ("pushing", "push"),
            ("commits", "commit"),
            ("committed", "commit"),
            ("process", "process"),
            ("processes", "process"),
            ("issue", "issu"),
            ("issues", "issu"),
            ("restarted", "restart"),
            ("https", "http"),
            ("installed", "install"),
            ("rules", "rule"),
            ("gateway", "gateway"),
        ] {
            assert_eq!(stem(w), s, "{w}");
        }
    }

    #[test]
    fn task_terms_drop_stop_words_numbers_and_repeats() {
        let t = task_terms("Push a commit to GitHub over HTTPS, then push 2 more commits");
        let words: Vec<&str> = t.iter().map(|t| t.word.as_str()).collect();
        assert_eq!(words, vec!["push", "commit", "github", "https", "more"]);
        assert_eq!(t[3].stem, "http");
        assert!(task_terms("the and of 42").is_empty());
    }

    #[test]
    fn bm25_prefers_rarer_terms_and_reports_matches() {
        let docs = vec![
            terms("restart the gateway service"),
            terms("the widget service has a port"),
            terms("widget widget widget"),
        ];
        let q: Vec<String> = task_terms("restart service")
            .into_iter()
            .map(|t| t.stem)
            .collect();
        let s = bm25(&q, &docs);
        assert_eq!(s[0].1, vec![0, 1]);
        assert_eq!(s[1].1, vec![1]);
        assert!(s[2].1.is_empty());
        assert_eq!(s[2].0, 0.0);
        assert!(s[0].0 > s[1].0);
    }
}
