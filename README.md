# clw-api

The CloudLab Works public HTTP API and its explorer app. Free, no login, nothing stored.

## Live

- API and explorer: <https://api.cloudlabworks.dev>
- OpenAPI 3.1: <https://api.cloudlabworks.dev/openapi.json>
- Health: <https://api.cloudlabworks.dev/healthz>

```sh
curl https://api.cloudlabworks.dev/v1/nc2/permissions/gcp/nodes-sa
curl -X POST https://api.cloudlabworks.dev/v1/receipts/check \
  -H 'Content-Type: application/json' \
  -d '{"claim":"Deployed the API","evidence":"wrangler deploy: Uploaded clw-api; curl /healthz returned 200"}'
```

- **NC2 permissions**: the IAM permission lists that Nutanix Cloud Clusters (NC2) onboarding
  needs on AWS, Azure and Google Cloud, extracted from the public Nutanix documentation by
  [nc2-iam-preflight](https://github.com/cloudlabworks/nc2-iam-preflight). Every response
  carries the source page URL, the page's last-updated date and the date it was fetched.
- **Receipt check**: deterministic checks of a "this work is done" claim against the evidence
  offered for it, written in Rust and compiled to WebAssembly. No model calls.
- **Receipt pack**: a local CLI that builds a token-capped context pack for a task from a
  directory of notes, with provenance and a score breakdown on every item. No model calls.

| part | what | language |
|---|---|---|
| [`crates/receipt-check`](crates/receipt-check) | checker library, `receipt-check` CLI, Wasm build | Rust |
| [`crates/receipt-pack`](crates/receipt-pack) | context-pack library and `receipt-pack` CLI (local only, not served) | Rust |
| [`worker`](worker) | the API on Cloudflare Workers (Hono), serves the app as static assets | TypeScript |
| [`app`](app) | CloudLab Works API Explorer (Vite, no framework) | TypeScript |
| [`openapi/openapi.yaml`](openapi/openapi.yaml) | OpenAPI 3.1 description of every route | YAML |
| [`apis.json`](apis.json) | APIs.json 0.23 entry, for merging into cloudlabworks.dev/apis.json | JSON |

## Endpoints

| method | path | returns |
|---|---|---|
| GET | `/healthz` | `{"status":"ok",...}`, `Cache-Control: no-store` |
| GET | `/v1/nc2/permissions` | index of clouds and roles with sources and list sizes |
| GET | `/v1/nc2/permissions/{cloud}/{role}` | every list for that role, with its source; 404 for an unknown cloud or role |
| POST | `/v1/receipts/check` | verdict and flags for `{"claim", "evidence"}`; 400 bad input, 413 over limits or over 64 KB |
| GET | `/openapi.json` | the OpenAPI document |
| GET | `/` | the explorer app |

Roles: `aws/onboarding-user`, `azure/nc2-app`, `gcp/console-sa`, `gcp/nodes-sa`.

GET responses carry `Cache-Control: public, max-age=3600` (errors and POST: `no-store`).
CORS allows any origin for GET and POST. Errors are JSON: `{"error": "...", "code": "..."}`.
Wrong methods on a known path get 405 with an `Allow` header.

### Rate limits

Per client IP: 60 requests per minute across the GET routes and 20 per minute for
`POST /v1/receipts/check`. Over the limit the API answers 429 with `Retry-After: 60` and
`{"code": "rate_limited"}`. The limits use the Cloudflare Workers Rate Limiting binding
(`ratelimits` in [`worker/wrangler.jsonc`](worker/wrangler.jsonc)); counts are kept per
Cloudflare location and are approximate, so treat them as a brake, not a quota. CORS
preflights are not counted, and the explorer's static files are served before the Worker
runs, so they are not counted either. Request bodies over 64 KB get 413 without being read
in full.

### Receipt check rules

| flag | raised when |
|---|---|
| `no_evidence` | evidence is empty or whitespace |
| `no_artifact` | no hash, URL, path, id, timestamp, command output or number in the evidence |
| `claim_terms_missing` | over 60% of the claim's terms never appear in the evidence |

No flags: `supported`. One of `no_artifact` / `claim_terms_missing`: `partial`. Both, or
`no_evidence`: `unsupported`. A `supported` verdict is a floor, not proof that the work is
done. Limits: claim 600 characters, evidence 4000 characters, body 64 KiB.

## Examples

```console
$ curl -s localhost:8787/v1/nc2/permissions/gcp/nodes-sa | jq '{source, count}'
{
  "source": {
    "url": "https://portal.nutanix.com/page/documents/details?targetId=Nutanix-Cloud-Clusters-Google-Cloud:nc2-clusters-google-cloud-appendix-role-permissions-c.html",
    "doc_last_updated": "2026-10-08",
    "fetched": "2026-10-09T20:38:51.395Z"
  },
  "count": 23
}

$ curl -s localhost:8787/v1/receipts/check -H 'Content-Type: application/json' \
    -d '{"claim":"updated the readme","evidence":"the readme was updated"}' | jq '{verdict, flags}'
{
  "verdict": "partial",
  "flags": [
    {
      "code": "no_artifact",
      "message": "evidence contains no concrete artifact (hash, URL, path, id, timestamp, command output, number)"
    }
  ]
}

$ curl -s -o /dev/null -w '%{http_code}\n' localhost:8787/v1/nc2/permissions/oci/admin
404
```

The CLI gives the same report offline:

```console
$ cargo run -q -p receipt-check -- --claim "tests pass" --evidence "test result: ok. 22 passed"
```

## Receipt pack

`receipt-pack` runs on your machine only; the API does not expose it. Before a task, it
reads a directory of Markdown and text notes and prints the most relevant items that fit a
token budget (default 2000):

```console
$ cargo run -q -p receipt-pack -- build --task "restart the gateway" --root ~/notes
$ cargo run -q -p receipt-pack -- build --task "restart the gateway" --root ~/notes \
    --format json --out pack.json
$ cargo run -q -p receipt-pack -- verify pack.json
```

- **Provenance**: every item has its relative path, line range, the SHA-256 of the excerpt
  and, in a git work tree, the last commit of the file. `verify` re-hashes each item and
  reports `FRESH`, `STALE` or `MISSING`, exiting 1 if any item is not fresh.
- **Kinds**: items are rules, decisions, facts, anti-patterns or evidence, from frontmatter
  `type:` and `tags:`, path conventions, or `Always` / `Never` / `Prefer` bullets. Anti-patterns
  that match the task go in a WARNING section at the top and are never cut for budget.
- **Why it came up**: each item shows BM25 lexical relevance with the matched terms, the kind
  weight, a recency factor and matching tags, and the total. There are no embeddings in this
  version; a `SemanticScorer` trait is the hook for a later local scorer.

Details, flags and defaults: [`crates/receipt-pack/README.md`](crates/receipt-pack/README.md).

## Run locally

Needs Rust with the `wasm32-unknown-unknown` target, `wasm-bindgen-cli` at the version pinned
in `crates/receipt-check/Cargo.toml` (0.2.130), and Node.js 22 or newer.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked

cargo test                       # Rust libraries and CLIs
./scripts/build-wasm.sh          # writes worker/src/wasm/ (git-ignored)

(cd app && npm ci && npm run build)            # writes app/dist/, served by the Worker
(cd worker && npm ci && npm test && npm run dev)  # wrangler dev --local on :8787
```

`wrangler dev --local` runs entirely on your machine and needs no Cloudflare account. For app
work, run `npm run dev` in `app/` too: Vite proxies API calls to the Worker on port 8787.

If your Rust comes from a package manager without rustup, install rustup into a separate
directory and point the build at it, for example
`RUSTUP_HOME=~/.rustup CARGO_HOME=~/.rustup/cargo PATH=~/.rustup/cargo/bin:$PATH ./scripts/build-wasm.sh`.

## Checks

CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs, and you can run locally:

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
./scripts/build-wasm.sh
(cd app && npm run typecheck && npm test && npm run build)
(cd worker && npm run check:openapi && npm run typecheck && npm test && npm run lint:openapi)
```

After editing `openapi/openapi.yaml`, run `npm run build:openapi` in `worker/` to refresh
`worker/src/generated/openapi.json`; a test fails if the two differ.

## Deployment

The Worker `clw-api` serves the Custom Domain `api.cloudlabworks.dev` (`routes` with
`custom_domain: true` in [`worker/wrangler.jsonc`](worker/wrangler.jsonc)); `workers.dev` is off.
Deploy with `npx wrangler deploy` in `worker/` after building the Wasm and the app.

## License

MIT. See [LICENSE](LICENSE). The permission lists are facts extracted from public Nutanix
documentation; the linked source pages are the authority.
