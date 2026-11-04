//! End-to-end tests of the `receipt-check` binary: exit codes, stdin and file input.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run(args: &[&str], stdin: Option<&str>) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_receipt-check"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("binary runs");
    let mut pipe = child.stdin.take().unwrap();
    pipe.write_all(stdin.unwrap_or("").as_bytes()).unwrap();
    drop(pipe);
    child.wait_with_output().unwrap()
}

fn report(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).expect("stdout is JSON")
}

#[test]
fn supported_exits_zero() {
    let out = run(
        &[
            "--claim",
            "tests pass",
            "--evidence",
            "$ cargo test\ntest result: ok. 14 passed",
        ],
        None,
    );
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(report(&out)["verdict"], "supported");
}

#[test]
fn partial_exits_one() {
    let out = run(
        &[
            "--claim",
            "updated the readme",
            "--evidence",
            "the readme was updated",
        ],
        None,
    );
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(report(&out)["flags"][0]["code"], "no_artifact");
}

#[test]
fn unsupported_exits_two_and_reads_stdin() {
    let out = run(&["--claim", "deployed", "--compact"], Some(""));
    assert_eq!(out.status.code(), Some(2));
    let stdout = String::from_utf8(out.stdout.clone()).unwrap();
    assert_eq!(stdout.trim().lines().count(), 1);
    assert_eq!(report(&out)["flags"][0]["code"], "no_evidence");
}

#[test]
fn evidence_file_is_read() {
    let dir = std::env::temp_dir().join(format!("receipt-check-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("evidence.txt");
    std::fs::write(&path, "deploy finished at 2026-11-03 10:00, HTTP/1.1 200").unwrap();
    let out = run(
        &[
            "--claim",
            "deployed",
            "--evidence-file",
            path.to_str().unwrap(),
        ],
        None,
    );
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(out.status.code(), Some(0));
    let r = report(&out);
    assert_eq!(
        r["artifacts"],
        serde_json::json!(["timestamp", "command", "number"])
    );
}

#[test]
fn oversize_evidence_from_stdin_is_rejected_not_truncated() {
    let big = "x".repeat(5000);
    let out = run(&["--claim", "wrote a lot"], Some(&big));
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(report(&out)["code"], "too_large");
}

#[test]
fn empty_claim_exits_three() {
    let out = run(&["--claim", "  ", "--evidence", "exit 0"], None);
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(report(&out)["code"], "claim_required");
}

#[test]
fn usage_errors_exit_64() {
    assert_eq!(run(&[], None).status.code(), Some(64));
    assert_eq!(run(&["--bogus"], None).status.code(), Some(64));
    assert_eq!(run(&["--claim"], None).status.code(), Some(64));
    let out = run(
        &["--claim", "x", "--evidence-file", "/nonexistent/receipt"],
        None,
    );
    assert_eq!(out.status.code(), Some(64));
}

#[test]
fn help_and_version() {
    let out = run(&["--help"], None);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("usage: receipt-check"));
    let out = run(&["--version"], None);
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("receipt-check "));
}
