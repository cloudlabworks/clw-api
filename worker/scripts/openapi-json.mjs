// Convert ../openapi/openapi.yaml to src/generated/openapi.json, which the Worker serves at
// /openapi.json. With --check, exit 1 instead of writing when the JSON is out of date.
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { parse } from "yaml";

const yamlPath = fileURLToPath(new URL("../../openapi/openapi.yaml", import.meta.url));
const jsonPath = fileURLToPath(new URL("../src/generated/openapi.json", import.meta.url));

const want = JSON.stringify(parse(readFileSync(yamlPath, "utf8")), null, 2) + "\n";
if (process.argv.includes("--check")) {
  let have = "";
  try {
    have = readFileSync(jsonPath, "utf8");
  } catch {
    // missing file is out of date
  }
  if (have !== want) {
    console.error("src/generated/openapi.json is out of date; run: npm run build:openapi");
    process.exit(1);
  }
  console.log("openapi.json is up to date");
} else {
  writeFileSync(jsonPath, want);
  console.log(`wrote ${jsonPath}`);
}
