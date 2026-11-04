import { api, ApiError, type CheckReport } from "./api";
import { clear, h } from "./dom";
import { charCount } from "./filter";

const MAX_CLAIM = 600;
const MAX_EVIDENCE = 4000;

const SAMPLES: Array<{ label: string; claim: string; evidence: string }> = [
  {
    label: "Well evidenced",
    claim: "Tests pass on main",
    evidence: "$ cargo test\ntest result: ok. 22 passed; 0 failed\nmain at 3f9a2b1",
  },
  {
    label: "No artifact",
    claim: "Updated the readme",
    evidence: "The readme was updated.",
  },
  {
    label: "Off topic",
    claim: "Rotated the database credentials",
    evidence: "Looks good to me, shipped it.",
  },
];

const VERDICT_TEXT: Record<CheckReport["verdict"], string> = {
  supported: "Supported: the evidence passed every deterministic check.",
  partial: "Partial: one check flagged the evidence.",
  unsupported: "Unsupported: the evidence is missing or failed both checks.",
};

function counter(field: HTMLTextAreaElement, out: HTMLElement, max: number): void {
  const update = () => {
    const n = charCount(field.value);
    out.textContent = `${n} / ${max} characters`;
    out.classList.toggle("over", n > max);
  };
  field.addEventListener("input", update);
  update();
}

function list(title: string, items: string[], empty: string): HTMLElement {
  return h(
    "div",
    { class: "result-part" },
    h("h3", {}, title),
    items.length ? h("ul", { class: "chips" }, ...items.map((i) => h("li", {}, h("code", {}, i)))) : h("p", { class: "empty" }, empty),
  );
}

export function renderReceipts(main: HTMLElement): void {
  const claim = h("textarea", { id: "claim", name: "claim", rows: 3, required: true, "aria-describedby": "claim-count" });
  const evidence = h("textarea", { id: "evidence", name: "evidence", rows: 10, "aria-describedby": "evidence-help evidence-count" });
  const claimCount = h("small", { id: "claim-count", class: "count" });
  const evidenceCount = h("small", { id: "evidence-count", class: "count" });
  const submit = h("button", { type: "submit" }, "Check");
  const result = h("section", { class: "result", "aria-live": "polite", "aria-labelledby": "result-title" });

  const samples = h(
    "p",
    { class: "samples" },
    "Try a sample: ",
    ...SAMPLES.map((s) => {
      const b = h("button", { type: "button", class: "secondary" }, s.label);
      b.addEventListener("click", () => {
        claim.value = s.claim;
        evidence.value = s.evidence;
        claim.dispatchEvent(new Event("input"));
        evidence.dispatchEvent(new Event("input"));
        claim.focus();
      });
      return b;
    }),
  );

  const form = h(
    "form",
    { class: "check-form", novalidate: true },
    h("div", { class: "field" }, h("label", { for: "claim" }, "Claim"), claim, claimCount),
    h(
      "div",
      { class: "field" },
      h("label", { for: "evidence" }, "Evidence"),
      evidence,
      h("small", { id: "evidence-help" }, "Paste the command output, commit, URL or log lines offered as proof."),
      evidenceCount,
    ),
    submit,
  );

  counter(claim, claimCount, MAX_CLAIM);
  counter(evidence, evidenceCount, MAX_EVIDENCE);

  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    clear(result);
    const title = h("h2", { id: "result-title" }, "Result");
    if (!claim.value.trim()) {
      result.append(title, h("p", { class: "error" }, "Enter a claim to check."));
      claim.focus();
      return;
    }
    submit.disabled = true;
    result.append(title, h("p", {}, "Checking..."));
    try {
      const report = await api.check(claim.value, evidence.value);
      clear(result);
      result.append(
        title,
        h("p", { class: `verdict verdict-${report.verdict}` }, h("strong", {}, report.verdict), " ", VERDICT_TEXT[report.verdict]),
        h(
          "div",
          { class: "result-part" },
          h("h3", {}, "Flags"),
          report.flags.length
            ? h("ul", { class: "flags" }, ...report.flags.map((f) => h("li", {}, h("code", {}, f.code), ": ", f.message)))
            : h("p", { class: "empty" }, "No flags."),
        ),
        list("Artifacts found", report.artifacts, "None."),
        list("Claim terms missing from the evidence", report.missing_terms, "None."),
        h("p", { class: "fineprint" }, `${report.checker}. No model was called and nothing was stored. A supported verdict is a floor, not proof.`),
      );
    } catch (err) {
      clear(result);
      const msg = err instanceof ApiError ? `${err.message}${err.status ? ` (HTTP ${err.status})` : ""}` : "something went wrong";
      result.append(title, h("p", { class: "error" }, `Could not check: ${msg}.`));
    } finally {
      submit.disabled = false;
    }
  });

  main.append(
    h("h1", { tabindex: -1 }, "Receipt check"),
    h(
      "p",
      { class: "lede" },
      "Paste a claim that some work is done and the evidence for it. The checker (Rust, compiled to WebAssembly, running in the API) flags missing evidence, evidence with no concrete artifact, and claims whose terms the evidence never mentions.",
    ),
    samples,
    form,
    result,
  );
}
