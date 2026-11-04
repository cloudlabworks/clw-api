import "./style.css";
import { clear } from "./dom";
import { routeFromHash, type Page } from "./filter";
import { renderPermissions } from "./permissions-page";
import { renderReceipts } from "./receipts-page";

const TITLES: Record<Page, string> = {
  permissions: "NC2 permissions",
  receipts: "Receipt check",
};

const main = document.getElementById("main") as HTMLElement;
let first = true;

function show(): void {
  const page = routeFromHash(location.hash);
  document.title = `${TITLES[page]} | CloudLab Works API Explorer`;
  for (const link of document.querySelectorAll<HTMLAnchorElement>("nav a[data-route]")) {
    if (link.dataset["route"] === page) link.setAttribute("aria-current", "page");
    else link.removeAttribute("aria-current");
  }
  clear(main);
  if (page === "receipts") renderReceipts(main);
  else void renderPermissions(main);
  // Move focus to the new page heading on navigation, not on first load.
  if (!first) main.querySelector<HTMLElement>("h1")?.focus();
  first = false;
}

window.addEventListener("hashchange", show);
show();
