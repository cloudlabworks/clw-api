import { Hono } from "hono";
import type { Context } from "hono";
import { cors } from "hono/cors";
import openapi from "./generated/openapi.json";
import { ROLES, summarize } from "./permissions";

export const VERSION = "0.1.0";

/** Largest request body accepted by POST /v1/receipts/check, in bytes. The checker's own
 * limits (600 chars claim, 4000 chars evidence) are far below this even with JSON escapes. */
export const MAX_BODY_BYTES = 64 * 1024;

export interface Env {
  ASSETS: Fetcher;
}

/** The Wasm `check(claim, evidence)` export: returns a JSON report or a JSON input error. */
export type Checker = (claim: string, evidence: string) => string;

const CACHE_STATIC = "public, max-age=3600";
const CACHE_NONE = "no-store";

type ErrorCode =
  | "not_found"
  | "method_not_allowed"
  | "invalid_json"
  | "invalid_body"
  | "claim_required"
  | "too_large"
  | "internal";

function error(c: Context, status: 400 | 404 | 405 | 413 | 500, code: ErrorCode, message: string, extra: Record<string, unknown> = {}) {
  c.header("Cache-Control", CACHE_NONE);
  return c.json({ error: message, code, ...extra }, status);
}

function methodNotAllowed(allow: string) {
  return (c: Context) => {
    c.header("Allow", `${allow}, OPTIONS`);
    return error(c, 405, "method_not_allowed", `method ${c.req.method} not allowed; use ${allow}`);
  };
}

export function createApp(check: Checker) {
  const app = new Hono<{ Bindings: Env }>();

  app.use(
    "*",
    cors({
      origin: "*",
      allowMethods: ["GET", "POST", "OPTIONS"],
      allowHeaders: ["Content-Type"],
      maxAge: 86400,
    }),
  );
  app.use("*", async (c, next) => {
    await next();
    c.header("X-Content-Type-Options", "nosniff");
  });

  app.get("/healthz", (c) => {
    c.header("Cache-Control", CACHE_NONE);
    return c.json({ status: "ok", service: "clw-api", version: VERSION });
  });

  app.get("/openapi.json", (c) => {
    c.header("Cache-Control", CACHE_STATIC);
    return c.json(openapi);
  });

  app.get("/v1/nc2/permissions", (c) => {
    c.header("Cache-Control", CACHE_STATIC);
    const clouds = [...ROLES.entries()].map(([cloud, roles]) => ({
      cloud,
      roles: [...roles.values()].map(summarize),
    }));
    return c.json({
      clouds,
      origin: "Public Nutanix NC2 documentation, extracted by nc2-iam-preflight",
    });
  });

  app.get("/v1/nc2/permissions/:cloud/:role", (c) => {
    const { cloud, role } = c.req.param();
    const roles = ROLES.get(cloud);
    if (!roles) {
      return error(c, 404, "not_found", `unknown cloud: ${cloud}`, { known: [...ROLES.keys()] });
    }
    const doc = roles.get(role);
    if (!doc) {
      return error(c, 404, "not_found", `unknown role for ${cloud}: ${role}`, { known: [...roles.keys()] });
    }
    c.header("Cache-Control", CACHE_STATIC);
    return c.json(doc);
  });

  app.post("/v1/receipts/check", async (c) => {
    const declared = Number(c.req.header("Content-Length") ?? "0");
    if (declared > MAX_BODY_BYTES) {
      return error(c, 413, "too_large", `request body over ${MAX_BODY_BYTES} bytes`);
    }
    const text = await c.req.text();
    if (new TextEncoder().encode(text).byteLength > MAX_BODY_BYTES) {
      return error(c, 413, "too_large", `request body over ${MAX_BODY_BYTES} bytes`);
    }
    let body: unknown;
    try {
      body = JSON.parse(text);
    } catch {
      return error(c, 400, "invalid_json", "body must be JSON");
    }
    if (typeof body !== "object" || body === null || Array.isArray(body)) {
      return error(c, 400, "invalid_body", 'body must be an object: {"claim": string, "evidence": string}');
    }
    const { claim, evidence } = body as Record<string, unknown>;
    if (typeof claim !== "string") {
      return error(c, 400, "invalid_body", "claim must be a string");
    }
    if (evidence !== undefined && evidence !== null && typeof evidence !== "string") {
      return error(c, 400, "invalid_body", "evidence must be a string");
    }

    let result: Record<string, unknown>;
    try {
      result = JSON.parse(check(claim, evidence ?? "")) as Record<string, unknown>;
    } catch {
      return error(c, 500, "internal", "receipt check failed");
    }
    if (result["code"] === "claim_required") {
      return error(c, 400, "claim_required", String(result["error"]), { limits: result["limits"] });
    }
    if (result["code"] === "too_large") {
      return error(c, 413, "too_large", String(result["error"]), { limits: result["limits"] });
    }
    c.header("Cache-Control", CACHE_NONE);
    return c.json(result);
  });

  app.all("/v1/receipts/check", methodNotAllowed("POST"));
  for (const path of ["/healthz", "/openapi.json", "/v1/nc2/permissions", "/v1/nc2/permissions/:cloud/:role"]) {
    app.all(path, methodNotAllowed("GET"));
  }

  // The explorer app. Cloudflare serves static assets before the Worker runs, so this only
  // handles requests the asset layer passed through.
  app.get("/", (c) => c.env.ASSETS.fetch(c.req.raw));

  app.notFound((c) => error(c, 404, "not_found", `no route for ${c.req.method} ${c.req.path}`));
  app.onError((_err, c) => error(c, 500, "internal", "internal error"));

  return app;
}
