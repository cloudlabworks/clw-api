// Pure helpers for the explorer, kept free of DOM access so they are unit-tested.

/** Case-insensitive match of every whitespace-separated word in `query`. */
export function matches(entry: string, query: string): boolean {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const text = entry.toLowerCase();
  return words.every((w) => text.includes(w));
}

/** Filter each named list by `query`, keeping list order and dropping nothing else. */
export function filterLists(lists: Record<string, string[]>, query: string): Record<string, string[]> {
  return Object.fromEntries(Object.entries(lists).map(([name, items]) => [name, items.filter((i) => matches(i, query))]));
}

/** Total entries across lists. */
export function total(lists: Record<string, string[]>): number {
  return Object.values(lists).reduce((n, l) => n + l.length, 0);
}

/** Length in Unicode code points, the unit the API's size limits use. */
export function charCount(text: string): number {
  return [...text].length;
}

/** Hash route ("#/receipts") to page name; unknown or empty routes fall back to permissions. */
export type Page = "permissions" | "receipts";

export function routeFromHash(hash: string): Page {
  return hash.replace(/^#\/?/, "").split(/[/?]/)[0] === "receipts" ? "receipts" : "permissions";
}
