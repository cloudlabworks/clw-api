import { describe, expect, it, vi } from "vitest";
import { parse } from "yaml";
import specYaml from "../../openapi/openapi.yaml?raw";
import awsRaw from "../data/permissions/aws-onboarding-user-doc.json";
import azureRaw from "../data/permissions/azure-nc2-app.json";
import gcpConsoleRaw from "../data/permissions/gcp-console-sa.json";
import gcpNodesRaw from "../data/permissions/gcp-nodes-sa.json";
import worker from "../src/index";
import wranglerRaw from "../wrangler.jsonc?raw";
import { createApp, MAX_BODY_BYTES, RATE_LIMITS, type Env, type RateLimiter } from "../src/app";

type Json = Record<string, any>;

const assetsFetch = vi.fn(async () => new Response("<!doctype html><title>explorer</title>", { headers: { "Content-Type": "text/html" } }));
const env = { ASSETS: { fetch: assetsFetch } } as unknown as Env;

function get(path: string, init: RequestInit = {}) {
  return worker.request(path, init, env);
}

function postCheck(body: unknown, init: RequestInit = {}) {
  return worker.request(
    "/v1/receipts/check",
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: typeof body === "string" ? body : JSON.stringify(body),
      ...init,
    },
    env,
  );
}

async function json(res: Response): Promise<Json> {
  expect(res.headers.get("Content-Type")).toMatch(/^application\/json/);
  return (await res.json()) as Json;
}

const spec = parse(specYaml) as Json;

describe("GET /healthz", () => {
  it("reports ok, uncached, with CORS", async () => {
    const res = await get("/healthz", { headers: { Origin: "https://example.org" } });
    expect(res.status).toBe(200);
    expect(await json(res)).toEqual({ status: "ok", service: "clw-api", version: "0.1.0" });
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    expect(res.headers.get("Access-Control-Allow-Origin")).toBe("*");
    expect(res.headers.get("X-Content-Type-Options")).toBe("nosniff");
  });

  it("rejects other methods with 405 and Allow", async () => {
    const res = await get("/healthz", { method: "DELETE" });
    expect(res.status).toBe(405);
    expect(res.headers.get("Allow")).toBe("GET, OPTIONS");
    expect((await json(res)).code).toBe("method_not_allowed");
  });
});

describe("CORS preflight", () => {
  it("allows GET and POST from any origin", async () => {
    const res = await get("/v1/receipts/check", {
      method: "OPTIONS",
      headers: { Origin: "https://example.org", "Access-Control-Request-Method": "POST", "Access-Control-Request-Headers": "content-type" },
    });
    expect(res.status).toBe(204);
    expect(res.headers.get("Access-Control-Allow-Origin")).toBe("*");
    expect(res.headers.get("Access-Control-Allow-Methods")).toContain("GET");
    expect(res.headers.get("Access-Control-Allow-Methods")).toContain("POST");
    expect(res.headers.get("Access-Control-Allow-Headers")?.toLowerCase()).toContain("content-type");
  });
});

describe("GET /openapi.json", () => {
  it("serves the YAML spec as JSON, unchanged", async () => {
    const res = await get("/openapi.json");
    expect(res.status).toBe(200);
    expect(res.headers.get("Cache-Control")).toBe("public, max-age=3600");
    const body = await json(res);
    expect(body.openapi).toBe("3.1.0");
    expect(body).toEqual(spec);
  });

  it("documents every route the Worker serves", () => {
    expect(Object.keys(spec.paths).sort()).toEqual(
      ["/healthz", "/openapi.json", "/v1/nc2/permissions", "/v1/nc2/permissions/{cloud}/{role}", "/v1/receipts/check"].sort(),
    );
  });

  it("rejects POST with 405", async () => {
    expect((await get("/openapi.json", { method: "POST" })).status).toBe(405);
  });
});

const RAW: Record<string, Record<string, Json>> = {
  aws: { "onboarding-user": awsRaw },
  azure: { "nc2-app": azureRaw },
  gcp: { "console-sa": gcpConsoleRaw, "nodes-sa": gcpNodesRaw },
};

describe("GET /v1/nc2/permissions", () => {
  it("indexes every cloud and role with provenance", async () => {
    const res = await get("/v1/nc2/permissions");
    expect(res.status).toBe(200);
    expect(res.headers.get("Cache-Control")).toBe("public, max-age=3600");
    const body = await json(res);
    const index = Object.fromEntries(body.clouds.map((c: Json) => [c.cloud, c.roles.map((r: Json) => r.role)]));
    expect(index).toEqual({ aws: ["onboarding-user"], azure: ["nc2-app"], gcp: ["console-sa", "nodes-sa"] });
    for (const cloud of body.clouds) {
      for (const role of cloud.roles) {
        const raw = RAW[cloud.cloud]![role.role]!;
        expect(role.href).toBe(`/v1/nc2/permissions/${cloud.cloud}/${role.role}`);
        expect(role.source).toEqual({ url: raw.url, doc_last_updated: raw.doc_last_updated, fetched: raw.fetched });
        expect(role.actor).toBe(raw.actor);
      }
    }
  });

  it("matches the path enums in the spec", () => {
    const params = spec.paths["/v1/nc2/permissions/{cloud}/{role}"].get.parameters as Json[];
    expect(params.find((p) => p.name === "cloud")!.schema.enum).toEqual(Object.keys(RAW));
    expect(params.find((p) => p.name === "role")!.schema.enum.sort()).toEqual(
      Object.values(RAW).flatMap((roles) => Object.keys(roles)).sort(),
    );
  });
});

describe("GET /v1/nc2/permissions/{cloud}/{role}", () => {
  for (const [cloud, roles] of Object.entries(RAW)) {
    for (const [role, raw] of Object.entries(roles)) {
      it(`serves ${cloud}/${role} with every list and its source`, async () => {
        const res = await get(`/v1/nc2/permissions/${cloud}/${role}`);
        expect(res.status).toBe(200);
        expect(res.headers.get("Cache-Control")).toBe("public, max-age=3600");
        const body = await json(res);
        expect(body.cloud).toBe(cloud);
        expect(body.role).toBe(role);
        expect(body.actor).toBe(raw.actor);
        expect(body.origin).toBe(raw.origin);
        expect(body.source).toEqual({ url: raw.url, doc_last_updated: raw.doc_last_updated, fetched: raw.fetched });
        for (const [key, value] of Object.entries(raw)) {
          if (Array.isArray(value)) expect(body.lists[key]).toEqual(value);
        }
        const total = Object.values(raw).filter(Array.isArray).reduce((n, l) => n + l.length, 0);
        expect(body.count).toBe(total);
      });
    }
  }

  it("keeps scalar notes such as the doc-stated count", async () => {
    const body = await json(await get("/v1/nc2/permissions/gcp/console-sa"));
    expect(body.notes.doc_stated_count).toBe(gcpConsoleRaw.doc_stated_count);
    const aws = await json(await get("/v1/nc2/permissions/aws/onboarding-user"));
    expect(aws.notes.statement).toBe(awsRaw.statement);
  });

  it("404s an unknown cloud with the known clouds", async () => {
    const res = await get("/v1/nc2/permissions/oci/admin");
    expect(res.status).toBe(404);
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    expect(await json(res)).toEqual({ error: "unknown cloud: oci", code: "not_found", known: ["aws", "azure", "gcp"] });
  });

  it("404s an unknown role with the cloud's roles", async () => {
    const res = await get("/v1/nc2/permissions/gcp/admin");
    expect(res.status).toBe(404);
    expect(await json(res)).toEqual({ error: "unknown role for gcp: admin", code: "not_found", known: ["console-sa", "nodes-sa"] });
  });

  it("does not match a role from another cloud", async () => {
    expect((await get("/v1/nc2/permissions/aws/nodes-sa")).status).toBe(404);
  });
});

describe("POST /v1/receipts/check", () => {
  it("supports well-evidenced claims", async () => {
    const res = await postCheck({
      claim: "Tests pass on main",
      evidence: "$ cargo test\ntest result: ok. 22 passed; 0 failed\nmain at 3f9a2b1",
    });
    expect(res.status).toBe(200);
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    const body = await json(res);
    expect(body.verdict).toBe("supported");
    expect(body.flags).toEqual([]);
    expect(body.model_called).toBe(false);
    expect(body.checker).toMatch(/^receipt-check \d+\.\d+\.\d+$/);
  });

  it("flags no_artifact as partial", async () => {
    const body = await json(await postCheck({ claim: "updated the readme", evidence: "the readme was updated" }));
    expect(body.verdict).toBe("partial");
    expect(body.flags.map((f: Json) => f.code)).toEqual(["no_artifact"]);
  });

  it("flags claim_terms_missing as partial", async () => {
    const body = await json(await postCheck({ claim: "migrated database schema", evidence: "commit 3f9a2b1c pushed" }));
    expect(body.verdict).toBe("partial");
    expect(body.flags.map((f: Json) => f.code)).toEqual(["claim_terms_missing"]);
    expect(body.missing_terms).toEqual(["database", "migrated", "schema"]);
  });

  it("is unsupported when both soft flags fire", async () => {
    const body = await json(await postCheck({ claim: "rotated credentials", evidence: "trust me" }));
    expect(body.verdict).toBe("unsupported");
    expect(body.flags.map((f: Json) => f.code)).toEqual(["no_artifact", "claim_terms_missing"]);
  });

  it.each([[""], ["  \n"], [null], [undefined]])("is unsupported with no evidence (%j)", async (evidence) => {
    const body = await json(await postCheck({ claim: "deployed", evidence }));
    expect(body.verdict).toBe("unsupported");
    expect(body.flags).toEqual([{ code: "no_evidence", message: "no evidence given" }]);
  });

  it("400s a body that is not JSON", async () => {
    const res = await postCheck("{claim:");
    expect(res.status).toBe(400);
    expect(await json(res)).toEqual({ error: "body must be JSON", code: "invalid_json" });
  });

  it.each([[[]], [null], ["a string"], [42]])("400s a JSON body that is not an object (%j)", async (body) => {
    const res = await postCheck(JSON.stringify(body));
    expect(res.status).toBe(400);
    expect((await json(res)).code).toBe("invalid_body");
  });

  it("400s a missing or non-string claim", async () => {
    for (const body of [{ evidence: "x" }, { claim: 1, evidence: "x" }]) {
      const res = await postCheck(body);
      expect(res.status).toBe(400);
      expect(await json(res)).toEqual({ error: "claim must be a string", code: "invalid_body" });
    }
  });

  it("400s a non-string evidence", async () => {
    const res = await postCheck({ claim: "x", evidence: ["a"] });
    expect(res.status).toBe(400);
    expect(await json(res)).toEqual({ error: "evidence must be a string", code: "invalid_body" });
  });

  it("400s an empty claim", async () => {
    const res = await postCheck({ claim: "   ", evidence: "exit 0" });
    expect(res.status).toBe(400);
    expect(await json(res)).toEqual({ error: "claim is required", code: "claim_required", limits: { claim: 600, evidence: 4000 } });
  });

  it("413s a claim over 600 characters", async () => {
    expect((await postCheck({ claim: "é".repeat(600), evidence: "x" })).status).toBe(200);
    const res = await postCheck({ claim: "é".repeat(601), evidence: "x" });
    expect(res.status).toBe(413);
    expect(await json(res)).toEqual({
      error: "limits: claim 600 chars, evidence 4000 chars",
      code: "too_large",
      limits: { claim: 600, evidence: 4000 },
    });
  });

  it("413s evidence over 4000 characters", async () => {
    expect((await postCheck({ claim: "x claim", evidence: "y".repeat(4000) })).status).toBe(200);
    const res = await postCheck({ claim: "x claim", evidence: "y".repeat(4001) });
    expect(res.status).toBe(413);
    expect((await json(res)).code).toBe("too_large");
  });

  it("413s a body over the byte limit before parsing it", async () => {
    const res = await postCheck({ claim: "x", evidence: "y".repeat(MAX_BODY_BYTES) });
    expect(res.status).toBe(413);
    expect(await json(res)).toEqual({ error: `request body over ${MAX_BODY_BYTES} bytes`, code: "too_large" });
    const declared = await postCheck({ claim: "x" }, { headers: { "Content-Type": "application/json", "Content-Length": String(MAX_BODY_BYTES + 1) } });
    expect(declared.status).toBe(413);
  });

  it("405s GET with Allow: POST", async () => {
    const res = await get("/v1/receipts/check");
    expect(res.status).toBe(405);
    expect(res.headers.get("Allow")).toBe("POST, OPTIONS");
  });

  it("returns exactly the documented example responses", async () => {
    const op = spec.paths["/v1/receipts/check"].post;
    const requests = op.requestBody.content["application/json"].examples as Json;
    const responses = op.responses["200"].content["application/json"].examples as Json;
    const pairs: Array<[string, string]> = [
      ["supported", "supported"],
      ["noArtifact", "partial"],
    ];
    for (const [req, resp] of pairs) {
      const body = await json(await postCheck(requests[req].value));
      expect(body).toEqual(responses[resp].value);
    }
  });

  it("500s when the checker throws or returns garbage", async () => {
    for (const checker of [() => { throw new Error("boom"); }, () => "not json"]) {
      const app = createApp(checker);
      const res = await app.request("/v1/receipts/check", { method: "POST", headers: { "Content-Type": "application/json" }, body: '{"claim":"x","evidence":"y"}' }, env);
      expect(res.status).toBe(500);
      expect(await json(res)).toEqual({ error: "receipt check failed", code: "internal" });
    }
  });
});

describe("static app and unknown paths", () => {
  it("passes / through to the assets binding", async () => {
    assetsFetch.mockClear();
    const res = await get("/");
    expect(res.status).toBe(200);
    expect(await res.text()).toContain("explorer");
    expect(assetsFetch).toHaveBeenCalledOnce();
  });

  it("404s unknown paths with JSON", async () => {
    const res = await get("/v2/nothing");
    expect(res.status).toBe(404);
    expect(await json(res)).toEqual({ error: "no route for GET /v2/nothing", code: "not_found" });
  });
});

describe("request body cap", () => {
  it("413s a chunked body over the byte limit without a Content-Length", async () => {
    const chunk = new TextEncoder().encode("y".repeat(16 * 1024));
    let sent = 0;
    const stream = new ReadableStream<Uint8Array>({
      pull(controller) {
        if (sent > MAX_BODY_BYTES * 4) return controller.close();
        sent += chunk.byteLength;
        controller.enqueue(chunk);
      },
    });
    const res = await worker.request(
      "/v1/receipts/check",
      { method: "POST", headers: { "Content-Type": "application/json" }, body: stream, duplex: "half" } as RequestInit,
      env,
    );
    expect(res.status).toBe(413);
    expect((await json(res)).code).toBe("too_large");
    expect(sent).toBeLessThan(MAX_BODY_BYTES * 2);
  });
});

/** A stub of the Workers Rate Limiting binding: allows the first `budget` calls per key. */
function stubLimiter(budget: number) {
  const seen = new Map<string, number>();
  const keys: string[] = [];
  const limiter: RateLimiter = {
    limit: async ({ key }) => {
      keys.push(key);
      const n = (seen.get(key) ?? 0) + 1;
      seen.set(key, n);
      return { success: n <= budget };
    },
  };
  return { limiter, keys };
}

describe("rate limits", () => {
  function limitedEnv(readBudget: number, checkBudget: number) {
    const read = stubLimiter(readBudget);
    const check = stubLimiter(checkBudget);
    return { env: { ...env, RL_READ: read.limiter, RL_CHECK: check.limiter } as Env, read, check };
  }
  const ip = (addr: string) => ({ "CF-Connecting-IP": addr });

  it("429s over budget with Retry-After, CORS and a JSON error", async () => {
    const { env: e } = limitedEnv(1, 1);
    expect((await worker.request("/healthz", { headers: ip("203.0.113.7") }, e)).status).toBe(200);
    const res = await worker.request("/healthz", { headers: { ...ip("203.0.113.7"), Origin: "https://example.org" } }, e);
    expect(res.status).toBe(429);
    expect(res.headers.get("Retry-After")).toBe(String(RATE_LIMITS.read.period));
    expect(res.headers.get("Access-Control-Allow-Origin")).toBe("*");
    expect(res.headers.get("Access-Control-Expose-Headers")).toContain("Retry-After");
    expect(res.headers.get("Cache-Control")).toBe("no-store");
    expect(res.headers.get("X-Content-Type-Options")).toBe("nosniff");
    expect(await json(res)).toEqual({
      error: `rate limit: ${RATE_LIMITS.read.limit} requests per ${RATE_LIMITS.read.period} s per client IP; retry after ${RATE_LIMITS.read.period} s`,
      code: "rate_limited",
      limit: RATE_LIMITS.read.limit,
      period: RATE_LIMITS.read.period,
    });
  });

  it("keys on the client IP, so one client's budget does not block another", async () => {
    const { env: e, read } = limitedEnv(1, 1);
    expect((await worker.request("/v1/nc2/permissions", { headers: ip("203.0.113.7") }, e)).status).toBe(200);
    expect((await worker.request("/v1/nc2/permissions", { headers: ip("203.0.113.7") }, e)).status).toBe(429);
    expect((await worker.request("/v1/nc2/permissions", { headers: ip("198.51.100.9") }, e)).status).toBe(200);
    expect(read.keys).toEqual(["203.0.113.7", "203.0.113.7", "198.51.100.9"]);
  });

  it("counts receipt checks against the check budget and reads against the read budget", async () => {
    const { env: e, read, check } = limitedEnv(100, 1);
    const post = () =>
      worker.request("/v1/receipts/check", { method: "POST", headers: { ...ip("203.0.113.7"), "Content-Type": "application/json" }, body: '{"claim":"x","evidence":"y"}' }, e);
    expect((await post()).status).toBe(200);
    const limited = await post();
    expect(limited.status).toBe(429);
    expect(limited.headers.get("Retry-After")).toBe(String(RATE_LIMITS.check.period));
    expect((await json(limited)).limit).toBe(RATE_LIMITS.check.limit);
    expect((await worker.request("/healthz", { headers: ip("203.0.113.7") }, e)).status).toBe(200);
    expect(check.keys).toHaveLength(2);
    expect(read.keys).toHaveLength(1);
  });

  it("does not count CORS preflights", async () => {
    const { env: e, read, check } = limitedEnv(0, 0);
    const res = await worker.request(
      "/v1/receipts/check",
      { method: "OPTIONS", headers: { ...ip("203.0.113.7"), Origin: "https://example.org", "Access-Control-Request-Method": "POST" } },
      e,
    );
    expect(res.status).toBe(204);
    expect(read.keys).toHaveLength(0);
    expect(check.keys).toHaveLength(0);
  });

  it("passes requests when the binding is absent or errors", async () => {
    expect((await get("/healthz")).status).toBe(200);
    const broken: RateLimiter = { limit: async () => { throw new Error("binding down"); } };
    const res = await worker.request("/healthz", {}, { ...env, RL_READ: broken } as Env);
    expect(res.status).toBe(200);
  });

  it("matches the ratelimits bindings in wrangler.jsonc", () => {
    const config = JSON.parse(wranglerRaw.replace(/^\s*\/\/.*$/gm, "")) as Json;
    const byName = Object.fromEntries((config.ratelimits as Json[]).map((r) => [r.name, r]));
    for (const cls of Object.values(RATE_LIMITS)) {
      expect(byName[cls.binding]?.simple).toEqual({ limit: cls.limit, period: cls.period });
    }
    const ids = (config.ratelimits as Json[]).map((r) => r.namespace_id);
    expect(new Set(ids).size).toBe(ids.length);
    expect(config.routes).toEqual([{ pattern: "api.cloudlabworks.dev", custom_domain: true }]);
    expect(config.name).toBe("clw-api");
  });

  it("documents 429 with Retry-After on every operation", () => {
    for (const [path, ops] of Object.entries(spec.paths as Record<string, Json>)) {
      for (const [method, op] of Object.entries(ops)) {
        expect(op.responses["429"], `${method.toUpperCase()} ${path}`).toEqual({ $ref: "#/components/responses/RateLimited" });
      }
    }
    expect(spec.components.responses.RateLimited.headers["Retry-After"]).toBeDefined();
    expect(spec.components.schemas.Error.properties.code.enum).toContain("rate_limited");
  });
});
