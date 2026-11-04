// NC2 IAM permission lists, copied verbatim from nc2-iam-preflight
// (nc2_iam_preflight/permissions/*.json). Each file was extracted from public Nutanix
// documentation and records the page URL, the page's last-updated date and when it was fetched.

import awsOnboardingUser from "../data/permissions/aws-onboarding-user-doc.json";
import azureNc2App from "../data/permissions/azure-nc2-app.json";
import gcpConsoleSa from "../data/permissions/gcp-console-sa.json";
import gcpNodesSa from "../data/permissions/gcp-nodes-sa.json";

type RawFile = Record<string, unknown>;

export interface Source {
  url: string;
  doc_last_updated: string;
  fetched: string;
}

export interface RoleDoc {
  cloud: string;
  role: string;
  actor: string;
  origin: string;
  source: Source;
  /** Every list in the source file (permissions, actions, managed_policies, ...). */
  lists: Record<string, string[]>;
  /** Total entries across all lists. */
  count: number;
  /** Scalar fields the source file carries beyond the ones above (statement, doc_stated_count). */
  notes: Record<string, string | number>;
}

export interface RoleSummary {
  role: string;
  actor: string;
  href: string;
  source: Source;
  lists: Record<string, number>;
  count: number;
}

const FILES: ReadonlyArray<[cloud: string, role: string, raw: RawFile]> = [
  ["aws", "onboarding-user", awsOnboardingUser],
  ["azure", "nc2-app", azureNc2App],
  ["gcp", "console-sa", gcpConsoleSa],
  ["gcp", "nodes-sa", gcpNodesSa],
];

function str(raw: RawFile, key: string): string {
  const v = raw[key];
  if (typeof v !== "string") throw new Error(`permission file is missing string field ${key}`);
  return v;
}

function normalize(cloud: string, role: string, raw: RawFile): RoleDoc {
  const lists: Record<string, string[]> = {};
  const notes: Record<string, string | number> = {};
  const fixed = new Set(["cloud", "actor", "origin", "url", "doc_last_updated", "fetched"]);
  for (const [key, value] of Object.entries(raw)) {
    if (fixed.has(key)) continue;
    if (Array.isArray(value) && value.every((x) => typeof x === "string")) lists[key] = value;
    else if (typeof value === "string" || typeof value === "number") notes[key] = value;
    else throw new Error(`unexpected field ${key} in ${cloud}/${role}`);
  }
  if (str(raw, "cloud") !== cloud) throw new Error(`${cloud}/${role}: cloud field mismatch`);
  return {
    cloud,
    role,
    actor: str(raw, "actor"),
    origin: str(raw, "origin"),
    source: {
      url: str(raw, "url"),
      doc_last_updated: str(raw, "doc_last_updated"),
      fetched: str(raw, "fetched"),
    },
    lists,
    count: Object.values(lists).reduce((n, l) => n + l.length, 0),
    notes,
  };
}

export const ROLES: ReadonlyMap<string, ReadonlyMap<string, RoleDoc>> = (() => {
  const byCloud = new Map<string, Map<string, RoleDoc>>();
  for (const [cloud, role, raw] of FILES) {
    const roles = byCloud.get(cloud) ?? new Map<string, RoleDoc>();
    roles.set(role, normalize(cloud, role, raw));
    byCloud.set(cloud, roles);
  }
  return byCloud;
})();

export function summarize(doc: RoleDoc): RoleSummary {
  return {
    role: doc.role,
    actor: doc.actor,
    href: `/v1/nc2/permissions/${doc.cloud}/${doc.role}`,
    source: doc.source,
    lists: Object.fromEntries(Object.entries(doc.lists).map(([k, v]) => [k, v.length])),
    count: doc.count,
  };
}
