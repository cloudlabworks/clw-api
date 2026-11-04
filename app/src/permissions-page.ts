import { api, ApiError, type PermissionsIndex, type RoleDoc } from "./api";
import { clear, h } from "./dom";
import { filterLists, total } from "./filter";

function errorText(err: unknown): string {
  return err instanceof ApiError ? err.message : "something went wrong";
}

export async function renderPermissions(main: HTMLElement): Promise<void> {
  const status = h("p", { class: "status", role: "status", "aria-live": "polite" }, "Loading clouds and roles...");
  main.append(
    h("h1", { tabindex: -1 }, "NC2 permissions"),
    h(
      "p",
      { class: "lede" },
      "The IAM permissions Nutanix Cloud Clusters (NC2) onboarding needs on AWS, Azure and Google Cloud, as listed in the public Nutanix documentation. Each list links to its source page and shows when it was fetched.",
    ),
    status,
  );

  let index: PermissionsIndex;
  try {
    index = await api.permissionsIndex();
  } catch (err) {
    status.textContent = `Could not load the index: ${errorText(err)}.`;
    status.classList.add("error");
    return;
  }

  const cloudSelect = h("select", { id: "cloud", name: "cloud" });
  const roleSelect = h("select", { id: "role", name: "role" });
  const query = h("input", { id: "q", name: "q", type: "search", autocomplete: "off", placeholder: "e.g. compute.instances", "aria-describedby": "q-help" });
  const copy = h("button", { type: "button", class: "secondary" }, "Copy shown entries");
  const details = h("div", { class: "role-details" });

  for (const { cloud } of index.clouds) cloudSelect.append(h("option", { value: cloud }, cloud));

  const form = h(
    "form",
    { role: "search", class: "controls", "aria-label": "Choose and filter a permission list" },
    h("div", { class: "field" }, h("label", { for: "cloud" }, "Cloud"), cloudSelect),
    h("div", { class: "field" }, h("label", { for: "role" }, "Role"), roleSelect),
    h(
      "div",
      { class: "field grow" },
      h("label", { for: "q" }, "Filter entries"),
      query,
      h("small", { id: "q-help" }, "Matches every word, case-insensitive."),
    ),
  );
  form.addEventListener("submit", (e) => e.preventDefault());
  status.textContent = "";
  main.append(form, details);

  let doc: RoleDoc | undefined;
  let shown: Record<string, string[]> = {};

  function fillRoles(): void {
    clear(roleSelect);
    const entry = index.clouds.find((c) => c.cloud === cloudSelect.value);
    for (const r of entry?.roles ?? []) {
      roleSelect.append(h("option", { value: r.role }, `${r.role} (${r.count})`));
    }
  }

  function renderLists(): void {
    clear(details);
    if (!doc) return;
    shown = filterLists(doc.lists, query.value);
    const n = total(shown);
    status.textContent = query.value.trim()
      ? `${n} of ${doc.count} entries match "${query.value.trim()}" in ${doc.cloud} / ${doc.role}.`
      : `${doc.count} entries in ${doc.cloud} / ${doc.role}.`;

    const meta = h(
      "dl",
      { class: "meta" },
      h("dt", {}, "Applies to"),
      h("dd", {}, h("code", {}, doc.actor)),
      h("dt", {}, "Source page"),
      h("dd", {}, h("a", { href: doc.source.url, rel: "noopener noreferrer" }, "Nutanix documentation"), ` (${doc.origin})`),
      h("dt", {}, "Page last updated"),
      h("dd", {}, h("time", { datetime: doc.source.doc_last_updated }, doc.source.doc_last_updated)),
      h("dt", {}, "Fetched"),
      h("dd", {}, h("time", { datetime: doc.source.fetched }, doc.source.fetched)),
    );
    for (const [key, value] of Object.entries(doc.notes)) {
      meta.append(h("dt", {}, key.replace(/_/g, " ")), h("dd", {}, String(value)));
    }
    details.append(h("h2", {}, `${doc.cloud} / ${doc.role}`), meta, copy);

    for (const [name, items] of Object.entries(shown)) {
      const all = doc.lists[name]?.length ?? 0;
      const id = `list-${name}`;
      details.append(
        h("h3", { id }, `${name} `, h("span", { class: "count" }, query.value.trim() ? `${items.length} of ${all}` : String(all))),
        items.length
          ? h("ul", { class: "entries", "aria-labelledby": id }, ...items.map((i) => h("li", {}, h("code", {}, i))))
          : h("p", { class: "empty" }, all ? "No matches." : "Empty in the source."),
      );
    }
  }

  async function loadRole(): Promise<void> {
    const cloud = cloudSelect.value;
    const role = roleSelect.value;
    if (!cloud || !role) return;
    status.textContent = `Loading ${cloud} / ${role}...`;
    status.classList.remove("error");
    try {
      doc = await api.role(cloud, role);
    } catch (err) {
      doc = undefined;
      clear(details);
      status.textContent = `Could not load ${cloud} / ${role}: ${errorText(err)}.`;
      status.classList.add("error");
      return;
    }
    renderLists();
  }

  cloudSelect.addEventListener("change", () => {
    fillRoles();
    void loadRole();
  });
  roleSelect.addEventListener("change", () => void loadRole());
  query.addEventListener("input", renderLists);
  copy.addEventListener("click", async () => {
    const text = Object.values(shown).flat().join("\n");
    try {
      await navigator.clipboard.writeText(text);
      status.textContent = `Copied ${total(shown)} entries.`;
    } catch {
      status.textContent = "Copy failed; select the entries and copy them by hand.";
    }
  });

  fillRoles();
  await loadRole();
}
