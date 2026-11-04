//! End-to-end tests of the `receipt-pack` binary against the synthetic fixture workspace
//! in `tests/fixtures/ws`. Tests that change files work on a copy in the temp directory.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const PUSH: &str = "push a commit to GitHub over HTTPS";
const RESTART: &str = "restart the gateway";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ws")
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_receipt-pack"))
        .args(args)
        .output()
        .expect("binary runs")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

/// `build --format json` with a fixed date and no git, plus `extra` flags.
fn pack(task: &str, root: &Path, extra: &[&str]) -> Value {
    let root = root.to_str().unwrap();
    let mut args = vec![
        "build",
        "--task",
        task,
        "--root",
        root,
        "--format",
        "json",
        "--now",
        "2026-06-01",
        "--no-git",
    ];
    args.extend_from_slice(extra);
    let out = run(&args);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).expect("stdout is JSON")
}

fn all_items(p: &Value) -> Vec<&Value> {
    let w = p["warnings"].as_array().unwrap();
    w.iter().chain(p["items"].as_array().unwrap()).collect()
}

fn paths(items: &Value) -> Vec<&str> {
    items
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["path"].as_str().unwrap())
        .collect()
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), to).unwrap();
        }
    }
}

/// A fresh copy of the fixtures, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("receipt-pack-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        copy_dir(&fixtures(), &dir);
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn every_item_has_provenance_and_a_score_breakdown() {
    let p = pack(PUSH, &fixtures(), &[]);
    assert_eq!(p["model_called"], false);
    assert_eq!(
        p["terms"],
        serde_json::json!(["push", "commit", "github", "https"])
    );
    let items = all_items(&p);
    assert!(items.len() >= 4);
    for it in items {
        let s = &it["score"];
        assert!(s["lexical"]["score"].is_f64() || s["lexical"]["score"].is_u64());
        assert!(s["lexical"]["matched_terms"].is_array());
        assert!(s["kind_weight"].as_f64().unwrap() > 0.0);
        assert!(s["recency"]["factor"].as_f64().unwrap() >= 0.5);
        assert!(s["recency"]["source"].is_string());
        assert!(s["tag_match"]["score"].is_number());
        assert!(s["tag_match"]["matched_tags"].is_array());
        assert!(s["semantic"].is_null());
        let total = s["total"].as_f64().unwrap();
        let expect = (s["lexical"]["score"].as_f64().unwrap()
            + s["tag_match"]["score"].as_f64().unwrap())
            * s["kind_weight"].as_f64().unwrap()
            * s["recency"]["factor"].as_f64().unwrap();
        assert!((total - expect).abs() < 1e-3, "{total} vs {expect}");
        // Provenance.
        assert_eq!(it["sha256"].as_str().unwrap().len(), 64);
        let (start, end) = (
            it["lines"]["start"].as_u64().unwrap(),
            it["lines"]["end"].as_u64().unwrap(),
        );
        assert!(start >= 1 && end >= start);
        assert_eq!(
            it["id"],
            format!("{}#L{start}-L{end}", it["path"].as_str().unwrap())
        );
        assert!(it["git_commit"].is_null());
        assert!(it["excerpt"].is_string());
    }
    let ledger = p["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "evidence")
        .expect("ledger is in the pack");
    assert_eq!(
        ledger["score"]["lexical"]["matched_terms"],
        serde_json::json!(["push", "commit", "github", "https"])
    );
    assert_eq!(ledger["score"]["recency"]["source"], "filename");
}

#[test]
fn budget_is_enforced() {
    let big = pack(RESTART, &fixtures(), &[]);
    let small = pack(RESTART, &fixtures(), &["--budget-tokens", "150"]);
    for p in [&big, &small] {
        let used: u64 = all_items(p)
            .iter()
            .map(|i| i["tokens"].as_u64().unwrap())
            .sum();
        assert_eq!(p["budget"]["used"], used);
        assert!(used <= p["budget"]["tokens"].as_u64().unwrap());
        assert_eq!(p["budget"]["over_budget"], false);
    }
    assert_eq!(small["budget"]["tokens"], 150);
    assert!(small["counts"]["dropped_for_budget"].as_u64().unwrap() > 0);
    assert!(
        small["counts"]["included"].as_u64().unwrap() < big["counts"]["included"].as_u64().unwrap()
    );
    // The default budget is 2000.
    assert_eq!(big["budget"]["tokens"], 2000);
}

#[test]
fn matching_anti_patterns_come_first_even_over_budget() {
    let p = pack(PUSH, &fixtures(), &["--budget-tokens", "1"]);
    assert_eq!(
        paths(&p["warnings"]),
        vec![
            "memory/learnings/incidents/2026-03-02-token-in-remote-url.md",
            "memory/notes/force-push.md"
        ]
    );
    for w in p["warnings"].as_array().unwrap() {
        assert_eq!(w["kind"], "anti-pattern");
    }
    assert_eq!(p["warnings"][1]["kind_reason"], "tag:anti-pattern");
    assert!(p["items"].as_array().unwrap().is_empty());
    assert_eq!(p["budget"]["over_budget"], true);
    assert_eq!(p["budget"]["used"], p["budget"]["warning_tokens"]);

    let p = pack(RESTART, &fixtures(), &["--budget-tokens", "1"]);
    assert_eq!(
        paths(&p["warnings"]),
        vec!["memory/learnings/incidents/gateway-restart-loop.md"]
    );
    assert_eq!(
        p["warnings"][0]["kind_reason"],
        "path:memory/learnings/incidents/**"
    );
}

#[test]
fn markdown_puts_the_warning_section_at_the_top() {
    let root = fixtures();
    let out = run(&[
        "build",
        "--task",
        PUSH,
        "--root",
        root.to_str().unwrap(),
        "--now",
        "2026-06-01",
        "--no-git",
    ]);
    assert_eq!(out.status.code(), Some(0));
    let md = stdout(&out);
    assert!(md.starts_with("# Context pack\n"));
    let warn = md
        .find("## WARNING: anti-patterns that match this task")
        .unwrap();
    let items = md.find("## Items").unwrap();
    assert!(warn < items);
    assert!(md[warn..items].contains("token-in-remote-url.md"));
    assert!(md.contains("- score: "));
    assert!(md.contains("- matched terms: "));
    assert!(md.contains("sha256 `"));
    // No warnings, no WARNING section.
    let out = run(&[
        "build",
        "--task",
        "changelog plain text",
        "--root",
        root.to_str().unwrap(),
        "--no-git",
    ]);
    assert!(!stdout(&out).contains("## WARNING"));
}

#[test]
fn kinds_come_from_frontmatter_tags_paths_and_bullets() {
    let p = pack(PUSH, &fixtures(), &[]);
    let kind_of = |path: &str| -> Vec<(String, String)> {
        all_items(&p)
            .into_iter()
            .filter(|i| i["path"] == path)
            .map(|i| {
                (
                    i["kind"].as_str().unwrap().to_owned(),
                    i["kind_reason"].as_str().unwrap().to_owned(),
                )
            })
            .collect()
    };
    assert_eq!(
        kind_of("USER.md")[0],
        ("rule".into(), "bullet:always-never-prefer".into())
    );
    assert_eq!(
        kind_of("memory/tasks/2026-05-01-release-widget.md")[0].0,
        "evidence"
    );
    assert_eq!(
        kind_of("memory/decisions/remote-protocol.md")[0],
        ("decision".into(), "frontmatter:type".into())
    );
    // A hidden directory is never indexed.
    let all = serde_json::to_string(&p).unwrap();
    assert!(!all.contains(".cache"));
    assert_eq!(p["counts"]["files"], 8);
}

#[test]
fn kind_flags_override_and_replace_the_defaults() {
    // A user path rule beats the bullet heuristic (frontmatter type: would still win).
    let p = pack(
        PUSH,
        &fixtures(),
        &["--kind-path", "anti-pattern=*.md", "--max-warnings", "5"],
    );
    let user = p["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["path"] == "USER.md")
        .expect("USER.md rule bullet became a warning");
    assert_eq!(user["kind"], "anti-pattern");
    assert_eq!(user["kind_reason"], "path:*.md");
    assert_eq!(p["warnings"].as_array().unwrap().len(), 3);

    let p = pack(RESTART, &fixtures(), &["--no-default-kinds"]);
    assert!(p["warnings"].as_array().unwrap().is_empty());
    let loop_item = all_items(&p)
        .into_iter()
        .find(|i| i["path"] == "memory/learnings/incidents/gateway-restart-loop.md")
        .unwrap()
        .clone();
    assert_eq!(loop_item["kind"], "fact");

    let p = pack(
        PUSH,
        &fixtures(),
        &["--no-default-kinds", "--kind-tag", "rule=git"],
    );
    let force = all_items(&p)
        .into_iter()
        .find(|i| i["path"] == "memory/notes/force-push.md")
        .unwrap()
        .clone();
    assert_eq!(
        (force["kind"].as_str(), force["kind_reason"].as_str()),
        (Some("rule"), Some("tag:git"))
    );
}

#[test]
fn output_is_deterministic_and_sorted() {
    let root = fixtures();
    let args = [
        "build",
        "--task",
        PUSH,
        "--root",
        root.to_str().unwrap(),
        "--format",
        "json",
        "--now",
        "2026-06-01",
        "--no-git",
    ];
    let a = stdout(&run(&args));
    let b = stdout(&run(&args));
    assert_eq!(a, b);
    let p: Value = serde_json::from_str(&a).unwrap();
    for list in [&p["warnings"], &p["items"]] {
        let keys: Vec<(f64, String, u64)> = list
            .as_array()
            .unwrap()
            .iter()
            .map(|i| {
                (
                    i["score"]["total"].as_f64().unwrap(),
                    i["path"].as_str().unwrap().to_owned(),
                    i["lines"]["start"].as_u64().unwrap(),
                )
            })
            .collect();
        for w in keys.windows(2) {
            let ordered =
                w[0].0 > w[1].0 || (w[0].0 == w[1].0 && (&w[0].1, w[0].2) <= (&w[1].1, w[1].2));
            assert!(ordered, "{:?} before {:?}", w[0], w[1]);
        }
    }
}

fn build_to(root: &Path, task: &str, out: &Path) {
    let o = run(&[
        "build",
        "--task",
        task,
        "--root",
        root.to_str().unwrap(),
        "--format",
        "json",
        "--out",
        out.to_str().unwrap(),
        "--no-git",
    ]);
    assert_eq!(o.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&o.stderr).starts_with("receipt-pack: wrote "));
}

fn verify_json(pack: &Path) -> (Option<i32>, Value) {
    let o = run(&["verify", pack.to_str().unwrap(), "--format", "json"]);
    (o.status.code(), serde_json::from_slice(&o.stdout).unwrap())
}

fn status_of<'a>(report: &'a Value, path: &str) -> Vec<&'a str> {
    report["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["path"] == path)
        .map(|c| c["status"].as_str().unwrap())
        .collect()
}

#[test]
fn verify_reports_stale_after_an_edit() {
    let s = Scratch::new("stale");
    let pack_path = s.0.join("pack.json");
    build_to(&s.0, RESTART, &pack_path);

    let (code, r) = verify_json(&pack_path);
    assert_eq!(code, Some(0));
    assert_eq!(r["stale"], 0);
    assert_eq!(r["missing"], 0);
    assert_eq!(r["fresh"], r["checked"]);

    let file =
        s.0.join("memory/learnings/incidents/gateway-restart-loop.md");
    let body = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, body.replace("separate shell", "different shell")).unwrap();

    let (code, r) = verify_json(&pack_path);
    assert_eq!(code, Some(1));
    assert_eq!(r["stale"], 1);
    assert_eq!(
        status_of(&r, "memory/learnings/incidents/gateway-restart-loop.md"),
        vec!["STALE"]
    );
    assert!(status_of(&r, "memory/reference/gateway.md")
        .iter()
        .all(|s| *s == "FRESH"));
    let stale = r["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["status"] == "STALE")
        .unwrap();
    assert_ne!(stale["actual_sha256"], stale["expected_sha256"]);

    // Text output names each status.
    let o = run(&["verify", pack_path.to_str().unwrap()]);
    let text = stdout(&o);
    assert!(text.contains("STALE    memory/learnings/incidents/gateway-restart-loop.md:1-7"));
    assert!(text.contains("FRESH    memory/reference/gateway.md:"));
    assert!(text.trim_end().ends_with("1 stale, 0 missing"));
}

#[test]
fn verify_reports_stale_when_the_file_shrinks() {
    let s = Scratch::new("shrink");
    let pack_path = s.0.join("pack.json");
    build_to(&s.0, RESTART, &pack_path);
    std::fs::write(
        s.0.join("memory/learnings/incidents/gateway-restart-loop.md"),
        "# Gone\n",
    )
    .unwrap();
    let (code, r) = verify_json(&pack_path);
    assert_eq!(code, Some(1));
    let c = r["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["status"] == "STALE")
        .unwrap();
    assert!(c["detail"]
        .as_str()
        .unwrap()
        .starts_with("line range past end of file"));
}

#[test]
fn verify_reports_missing_file() {
    let s = Scratch::new("missing");
    let pack_path = s.0.join("pack.json");
    build_to(&s.0, RESTART, &pack_path);
    std::fs::remove_file(s.0.join("memory/reference/gateway.md")).unwrap();
    let (code, r) = verify_json(&pack_path);
    assert_eq!(code, Some(1));
    let st = status_of(&r, "memory/reference/gateway.md");
    assert!(!st.is_empty() && st.iter().all(|s| *s == "MISSING"));
    assert_eq!(r["missing"], st.len());
    assert_eq!(
        status_of(&r, "memory/learnings/incidents/gateway-restart-loop.md"),
        vec!["FRESH"]
    );
}

#[test]
fn verify_follows_root_override_and_moved_tree() {
    let s = Scratch::new("moved");
    let pack_path = std::env::temp_dir().join(format!(
        "receipt-pack-test-{}-moved.json",
        std::process::id()
    ));
    build_to(&s.0, PUSH, &pack_path);
    let o = run(&[
        "verify",
        pack_path.to_str().unwrap(),
        "--root",
        fixtures().to_str().unwrap(),
    ]);
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));
    std::fs::remove_file(&pack_path).ok();
}

#[test]
fn git_commit_is_recorded_in_a_git_work_tree() {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("git not installed; skipping");
        return;
    }
    let s = Scratch::new("git");
    let git = |args: &[&str]| {
        let o = Command::new("git")
            .arg("-C")
            .arg(&s.0)
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap()
    };
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "fixtures"]);
    let head = git(&["rev-parse", "HEAD"]).trim().to_owned();
    std::fs::write(
        s.0.join("USER.md"),
        std::fs::read_to_string(s.0.join("USER.md")).unwrap() + "\nmore\n",
    )
    .unwrap();

    let o = run(&[
        "build",
        "--task",
        PUSH,
        "--root",
        s.0.to_str().unwrap(),
        "--format",
        "json",
    ]);
    let p: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(p["git_head"], head);
    for it in all_items(&p) {
        assert_eq!(it["git_commit"], head);
        assert_eq!(it["git_modified"], it["path"] == "USER.md");
    }
}

#[test]
fn bad_input_exits_three() {
    let root = fixtures();
    let r = root.to_str().unwrap();
    let o = run(&["build", "--task", "the and of", "--root", r]);
    assert_eq!(o.status.code(), Some(3));
    let e: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(e["code"], "no_terms");
    let o = run(&[
        "build",
        "--task",
        PUSH,
        "--root",
        "/nonexistent/receipt-pack",
    ]);
    assert_eq!(o.status.code(), Some(3));
    let o = run(&["build", "--task", PUSH, "--root", r, "--now", "2026-02-30"]);
    assert_eq!(o.status.code(), Some(3));
    let s = Scratch::new("badpack");
    let bad = s.0.join("bad.json");
    std::fs::write(&bad, "{\"not\": \"a pack\"}").unwrap();
    assert_eq!(
        run(&["verify", bad.to_str().unwrap()]).status.code(),
        Some(3)
    );
    assert_eq!(
        run(&["verify", "/nonexistent/pack.json"]).status.code(),
        Some(3)
    );
}

#[test]
fn usage_errors_exit_64() {
    let r = fixtures();
    let r = r.to_str().unwrap();
    for args in [
        vec![],
        vec!["bogus"],
        vec!["build", "--root", r],
        vec!["build", "--task", "x"],
        vec!["build", "--task", "x", "--root", r, "--format", "yaml"],
        vec![
            "build",
            "--task",
            "x",
            "--root",
            r,
            "--budget-tokens",
            "lots",
        ],
        vec![
            "build",
            "--task",
            "x",
            "--root",
            r,
            "--kind-path",
            "banana=**",
        ],
        vec!["build", "--task", "x", "--root", r, "--kind-tag", "rule"],
        vec!["build", "--task", "x", "--root", r, "--bogus"],
        vec!["verify"],
        vec!["verify", "a.json", "--task", "x"],
    ] {
        assert_eq!(run(&args).status.code(), Some(64), "{args:?}");
    }
}

#[test]
fn help_and_version() {
    let out = run(&["--help"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(stdout(&out).starts_with("usage: receipt-pack build"));
    assert_eq!(run(&["build", "--help"]).status.code(), Some(0));
    assert!(stdout(&run(&["--version"])).starts_with("receipt-pack "));
}
