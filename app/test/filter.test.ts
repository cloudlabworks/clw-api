import { describe, expect, it } from "vitest";
import { charCount, filterLists, matches, routeFromHash, total } from "../src/filter";

describe("matches", () => {
  it("requires every word, ignoring case", () => {
    expect(matches("compute.instances.get", "COMPUTE get")).toBe(true);
    expect(matches("compute.instances.get", "compute list")).toBe(false);
    expect(matches("anything", "   ")).toBe(true);
  });
});

describe("filterLists", () => {
  it("filters each list and keeps every list name", () => {
    const lists = { permissions: ["compute.routes.create", "compute.zones.get"], optional: [] };
    expect(filterLists(lists, "routes")).toEqual({ permissions: ["compute.routes.create"], optional: [] });
    expect(total(filterLists(lists, ""))).toBe(2);
  });
});

describe("charCount", () => {
  it("counts code points like the API limits", () => {
    expect(charCount("é")).toBe(1);
    expect(charCount("😀")).toBe(1);
    expect("😀".length).toBe(2);
  });
});

describe("routeFromHash", () => {
  it("maps hashes to pages with permissions as the default", () => {
    expect(routeFromHash("#/receipts")).toBe("receipts");
    expect(routeFromHash("#receipts")).toBe("receipts");
    expect(routeFromHash("#/permissions")).toBe("permissions");
    expect(routeFromHash("")).toBe("permissions");
    expect(routeFromHash("#/nope")).toBe("permissions");
  });
});
