# receipt-check

Deterministic checks of a completion claim ("the tests pass", "deployed to staging") against
the evidence offered for it. No model calls, no network, no state: the same input always
gives the same report.

The rules are a port of `deterministic_checks` from the CloudLab Works done-claim checker
(Python, AWS Lambda + Bedrock). There they ran before one model call; here they stand alone.

## Flags and verdict

| flag | raised when |
|---|---|
| `no_evidence` | evidence is empty or whitespace |
| `no_artifact` | evidence has no hash, URL, path, id, timestamp, command output or number |
| `claim_terms_missing` | over 60% of the claim's terms (lowercased, stop words removed, crude suffix stemming) never appear in the evidence |

| flags raised | verdict |
|---|---|
| none | `supported` |
| `no_artifact` or `claim_terms_missing` | `partial` |
| both | `unsupported` |
| `no_evidence` | `unsupported` |

`supported` means the evidence passed every deterministic check. It is a floor, not proof.

Limits: claim 600 characters after trimming, evidence 4000 characters (Unicode scalar values,
the same unit as Python `len`). An empty claim or a larger input is an input error.

## Library

```rust
let report = receipt_check::check("tests pass", "$ cargo test\ntest result: ok. 22 passed")?;
assert_eq!(report.verdict, receipt_check::Verdict::Supported);
```

`check_json(claim, evidence)` returns the report (or the input error) as a JSON string.

## CLI

```console
$ receipt-check --claim "tests pass" --evidence-file test-output.txt
$ cargo test 2>&1 | tail -n 5 | receipt-check --claim "tests pass" --compact
```

Exit codes: 0 supported, 1 partial, 2 unsupported, 3 bad input, 64 usage error.

## WebAssembly

Built for `wasm32-unknown-unknown` with wasm-bindgen; the module exports one function,
`check(claim, evidence)`, returning the same JSON string as `check_json`. The Cloudflare
Worker in `../../worker` uses it. Build it with `../../scripts/build-wasm.sh`.
