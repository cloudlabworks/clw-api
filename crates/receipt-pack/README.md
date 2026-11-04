# receipt-pack

Context packs with provenance. Before a task, `receipt-pack build` reads a directory of notes
(Markdown and plain text) and prints the items most relevant to the task, cut to a token
budget. Every item says where it came from and why it came up. Later, `receipt-pack verify`
re-reads the files and tells you which items are still true to their source.

No model calls, no embeddings, no network: the same files and flags give the same pack.

## What an item carries

- **Provenance**: path relative to the root, line range, SHA-256 of the excerpt, and in a git
  work tree the last commit that touched the file and whether the file has changed since.
- **Kind**: `rule`, `decision`, `fact`, `anti-pattern` or `evidence`, and the rule that set it.
- **Score breakdown**:

  ```text
  total = (lexical + tag_match) x kind_weight x recency
  ```

  | part | how |
  |---|---|
  | `lexical` | BM25 (k1 1.2, b 0.75) of the task terms over the item's path, heading and text, with the matched terms listed |
  | `tag_match` | 1.0 per frontmatter tag that shares a word with the task (at most 3 count), with the tags listed |
  | `kind_weight` | anti-pattern 1.3, rule 1.2, decision 1.1, fact 1.0, evidence 0.8 |
  | `recency` | `0.5 + 0.5 x 2^(-age / half-life)`, half-life 30 days; the date is frontmatter `updated`, `last_updated`, `date` or `created`, else a `YYYY-MM-DD` in the file name, else the file's mtime |

  Task terms are lowercased words with stop words and numbers removed and a crude suffix
  strip (`pushes`, `pushed`, `pushing` all match `push`). Items with no matching term and no
  matching tag are not candidates.

## Items and kinds

Each file is split into items: one per section (a heading and what follows it), long sections
cut at blank lines into pieces of at most 40 lines, and every bullet that starts with
`Always`, `Never` or `Prefer` (bold or not) as an item of its own. Frontmatter is not part of
any item.

The kind is set by the first rule that matches:

1. frontmatter `type:` (or `kind:`), with aliases such as `incident`, `ledger`, `adr`, `reference`
2. tag rules: tags `anti-pattern`, `antipattern` or `incident` make an anti-pattern
3. path rules: `memory/learnings/incidents/**` is anti-pattern, `memory/tasks/*.md` is evidence,
   `**/decisions/**` is decision
4. an `Always` / `Never` / `Prefer` bullet is a rule
5. anything else is a fact

`--kind-path KIND=GLOB` and `--kind-tag KIND=TAG` add rules ahead of the defaults;
`--no-default-kinds` drops the defaults in steps 2 and 3. Globs use `/`, `*` within a path
segment, `?` for one character and `**` for any number of segments.

## Anti-patterns first

An anti-pattern that matches at least two task terms (or all of them, for a one-word task),
or has a tag that matches, goes in the WARNING section at the top of the pack. Up to three do
(`--max-warnings`). The warnings are placed before the budget is applied and are never cut:
if they alone exceed the budget, the pack keeps them, holds no other items and reports
`over_budget: true`.

## Budget

Tokens are estimated as `ceil(characters / 4)` of each excerpt; metadata is not counted. The
budget (default 2000) covers warnings and items together. Items are taken in score order
(ties by path, then line); an item that does not fit is skipped and smaller ones below it may
still fit.

## CLI

```console
$ receipt-pack build --task "restart the gateway" --root ~/notes
$ receipt-pack build --task "restart the gateway" --root ~/notes \
    --format json --budget-tokens 1500 --exclude 'memory/logs/**' --out pack.json
$ receipt-pack verify pack.json
FRESH    memory/learnings/incidents/gateway-restart-loop.md:1-7
STALE    memory/reference/gateway.md:11-14
2 items: 1 fresh, 1 stale, 0 missing
```

`build` prints Markdown by default; `verify` needs the JSON form (`--format json`). `verify`
resolves paths against the root recorded in the pack, or `--root DIR`. An item is `STALE` when
its lines hash differently or the file is now shorter than the range, and `MISSING` when the
file is gone or the recorded path would leave the root.

Exit codes: 0 ok (`verify`: every item fresh), 1 stale or missing items, 3 bad input (no task
terms, root not a directory, bad `--now` date, unreadable pack), 64 usage error. Run
`receipt-pack --help` for every flag.

The walk skips symlinks, dot-files and dot-directories, `node_modules`, `target`, files over
1 MiB and files that are not valid UTF-8. Dates are UTC; `--now YYYY-MM-DD` fixes "today" for
reproducible packs.

## Library

```rust
let mut opts = receipt_pack::Options::new("restart the gateway", "notes");
opts.budget_tokens = 1500;
let pack = receipt_pack::build(&opts)?;
println!("{}", receipt_pack::markdown(&pack));
let report = receipt_pack::verify(&pack, std::path::Path::new(&pack.root));
assert!(report.ok());
```

## Semantic scoring hook

`SemanticScorer` is the place for a later local-embedding scorer: implement `name()` and
`similarity(task, text) -> [0, 1]` and pass it to `build_with`. Its similarity times 2.0 is
added next to `lexical` and `tag_match`, and shows in each item as `score.semantic` (always
`null` in this version, which ships no scorer). Keep it local and deterministic so packs stay
reproducible.

## Dependencies

| crate | why |
|---|---|
| `serde` | derive the pack and report types |
| `serde_json` | read and write packs; the JSON form is what `verify` checks |
| `sha2` | SHA-256 of each excerpt (RustCrypto, no default features) |

Argument parsing, the directory walk, globs, frontmatter, BM25 and date arithmetic are plain
standard library code, as in `receipt-check`. Git provenance shells out to `git` (read-only
`rev-parse`, `status` and `log`); without git, or outside a work tree, items carry no commit.

## Tests

`cargo test -p receipt-pack` runs unit tests and CLI tests against the synthetic notes in
`tests/fixtures/ws`. Tests that edit or delete files work on a copy in the temp directory.
