//! `receipt-check`: check a completion claim against its evidence from the command line.
//!
//! Prints the JSON report on stdout. Exit codes: 0 supported, 1 partial, 2 unsupported,
//! 3 bad input (empty claim or over the size limits), 64 usage error.

use receipt_check::{check, Verdict, MAX_CLAIM, MAX_EVIDENCE};
use std::io::Read;
use std::process::ExitCode;

const USAGE: &str = "\
usage: receipt-check --claim TEXT [--evidence TEXT | --evidence-file PATH]
       receipt-check --claim-file PATH [--evidence TEXT | --evidence-file PATH]

Checks a completion claim against its evidence with deterministic rules only
(no model calls, no network). Without --evidence or --evidence-file, evidence
is read from stdin. Use --evidence-file - to read stdin explicitly.

Prints a JSON report. Exit codes: 0 supported, 1 partial, 2 unsupported,
3 bad input (empty claim or over the size limits), 64 usage error.

options:
  --claim TEXT          the claim, e.g. \"deployed v1.2 to staging\"
  --claim-file PATH     read the claim from a file
  --evidence TEXT       the evidence text
  --evidence-file PATH  read the evidence from a file (- for stdin)
  --compact             print the JSON on one line
  -h, --help            show this help
  -V, --version         show the version";

struct Args {
    claim: String,
    evidence: String,
    compact: bool,
}

/// Read at most `limit` characters' worth of bytes (plus one, so oversize input is
/// still reported as too large instead of being silently truncated to fit).
fn read_bounded(mut r: impl Read, max_chars: usize) -> std::io::Result<String> {
    let cap = (max_chars as u64 + 1) * 4;
    let mut buf = Vec::new();
    r.by_ref().take(cap).read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn read_source(path: &str, max_chars: usize) -> Result<String, String> {
    if path == "-" {
        return read_bounded(std::io::stdin().lock(), max_chars)
            .map_err(|e| format!("reading stdin: {e}"));
    }
    let f = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    read_bounded(f, max_chars).map_err(|e| format!("{path}: {e}"))
}

enum Parsed {
    Run(Args),
    Exit(ExitCode),
}

fn parse(argv: Vec<String>) -> Result<Parsed, String> {
    let mut claim = None;
    let mut evidence = None;
    let mut compact = false;
    let mut it = argv.into_iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(Parsed::Exit(ExitCode::SUCCESS));
            }
            "-V" | "--version" => {
                println!("{}", receipt_check::CHECKER);
                return Ok(Parsed::Exit(ExitCode::SUCCESS));
            }
            "--compact" => compact = true,
            "--claim" => claim = Some(value("--claim")?),
            "--claim-file" => claim = Some(read_source(&value("--claim-file")?, MAX_CLAIM)?),
            "--evidence" => evidence = Some(value("--evidence")?),
            "--evidence-file" => {
                evidence = Some(read_source(&value("--evidence-file")?, MAX_EVIDENCE)?)
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    let claim = claim.ok_or("--claim or --claim-file is required")?;
    let evidence = match evidence {
        Some(e) => e,
        None => read_source("-", MAX_EVIDENCE)?,
    };
    Ok(Parsed::Run(Args {
        claim,
        evidence,
        compact,
    }))
}

fn to_json<T: serde::Serialize>(value: &T, compact: bool) -> String {
    let out = if compact {
        serde_json::to_string(value)
    } else {
        serde_json::to_string_pretty(value)
    };
    out.expect("report serializes")
}

fn main() -> ExitCode {
    let args = match parse(std::env::args().skip(1).collect()) {
        Ok(Parsed::Run(a)) => a,
        Ok(Parsed::Exit(code)) => return code,
        Err(msg) => {
            eprintln!("receipt-check: {msg}\n\n{USAGE}");
            return ExitCode::from(64);
        }
    };
    match check(&args.claim, &args.evidence) {
        Ok(report) => {
            println!("{}", to_json(&report, args.compact));
            ExitCode::from(match report.verdict {
                Verdict::Supported => 0,
                Verdict::Partial => 1,
                Verdict::Unsupported => 2,
            })
        }
        Err(err) => {
            println!("{}", to_json(&err, args.compact));
            eprintln!("receipt-check: {err}");
            ExitCode::from(3)
        }
    }
}
