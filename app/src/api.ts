// Typed client for the CloudLab Works API. Same origin by default; set VITE_API_BASE at build
// time to point the explorer at another host.

const BASE = (import.meta.env["VITE_API_BASE"] as string | undefined)?.replace(/\/$/, "") ?? "";

export interface Source {
  url: string;
  doc_last_updated: string;
  fetched: string;
}

export interface RoleSummary {
  role: string;
  actor: string;
  href: string;
  source: Source;
  lists: Record<string, number>;
  count: number;
}

export interface PermissionsIndex {
  origin: string;
  clouds: Array<{ cloud: string; roles: RoleSummary[] }>;
}

export interface RoleDoc {
  cloud: string;
  role: string;
  actor: string;
  origin: string;
  source: Source;
  lists: Record<string, string[]>;
  count: number;
  notes: Record<string, string | number>;
}

export type FlagCode = "no_evidence" | "no_artifact" | "claim_terms_missing";

export interface CheckReport {
  verdict: "supported" | "partial" | "unsupported";
  flags: Array<{ code: FlagCode; message: string }>;
  artifacts: string[];
  claim_terms: string[];
  missing_terms: string[];
  model_called: false;
  checker: string;
}

export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
    readonly code?: string,
  ) {
    super(message);
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  let res: Response;
  try {
    res = await fetch(BASE + path, init);
  } catch {
    throw new ApiError(0, "could not reach the API");
  }
  const body: unknown = await res.json().catch(() => null);
  if (!res.ok) {
    const err = (body ?? {}) as { error?: unknown; code?: unknown };
    const message = typeof err.error === "string" ? err.error : `HTTP ${res.status}`;
    throw new ApiError(res.status, message, typeof err.code === "string" ? err.code : undefined);
  }
  return body as T;
}

export const api = {
  permissionsIndex: () => request<PermissionsIndex>("/v1/nc2/permissions"),
  role: (cloud: string, role: string) =>
    request<RoleDoc>(`/v1/nc2/permissions/${encodeURIComponent(cloud)}/${encodeURIComponent(role)}`),
  check: (claim: string, evidence: string) =>
    request<CheckReport>("/v1/receipts/check", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ claim, evidence }),
    }),
};
