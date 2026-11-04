//! Git provenance through the `git` command line. Read-only: `rev-parse`, `status`, `log`.
//!
//! When the root is not inside a git work tree, or `git` is not installed, [`Repo::open`]
//! returns `None` and items carry no commit.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub struct Repo {
    root: PathBuf,
    /// HEAD commit, if the repository has one.
    pub head: Option<String>,
    /// Root-relative paths that differ from HEAD or are untracked.
    modified: BTreeSet<String>,
    commits: HashMap<String, Option<String>>,
}

impl Repo {
    pub fn open(root: &Path) -> Option<Repo> {
        if git(root, &["rev-parse", "--is-inside-work-tree"])?.trim() != "true" {
            return None;
        }
        // Root's path inside the repository, e.g. "docs/" (empty at the top level).
        let prefix = git(root, &["rev-parse", "--show-prefix"])?
            .trim()
            .to_owned();
        let head = git(root, &["rev-parse", "--verify", "-q", "HEAD"])
            .map(|h| h.trim().to_owned())
            .filter(|h| !h.is_empty());
        let status = git(
            root,
            &[
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--",
                ".",
            ],
        )
        .unwrap_or_default();
        Some(Repo {
            root: root.to_path_buf(),
            head,
            modified: parse_status(&status, &prefix),
            commits: HashMap::new(),
        })
    }

    /// Last commit that touched `rel`, or `None` if it was never committed.
    pub fn commit(&mut self, rel: &str) -> Option<String> {
        if let Some(c) = self.commits.get(rel) {
            return c.clone();
        }
        let spec = format!(":(literal){rel}");
        let c = git(&self.root, &["log", "-1", "--format=%H", "--", &spec])
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty());
        self.commits.insert(rel.to_owned(), c.clone());
        c
    }

    /// The working-tree file differs from HEAD, or is untracked.
    pub fn modified(&self, rel: &str) -> bool {
        self.modified.contains(rel)
    }
}

/// Paths from `git status --porcelain=v1 -z`, made relative to the root. Porcelain paths
/// are relative to the repository top level; a rename entry is followed by its old path.
fn parse_status(status: &str, prefix: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut entries = status.split('\0');
    while let Some(e) = entries.next() {
        if e.len() < 4 {
            continue;
        }
        let (xy, path) = e.split_at(3);
        if xy.contains(['R', 'C']) {
            entries.next();
        }
        if let Some(rel) = path.strip_prefix(prefix) {
            out.insert(rel.to_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_paths_are_root_relative() {
        let s = " M docs/a.md\0?? docs/new/b.md\0R  docs/c.md\0docs/old.md\0 M other/x.md\0";
        let got: Vec<String> = parse_status(s, "docs/").into_iter().collect();
        assert_eq!(got, vec!["a.md", "c.md", "new/b.md"]);
        let top: Vec<String> = parse_status(" M a.md\0", "").into_iter().collect();
        assert_eq!(top, vec!["a.md"]);
    }
}
