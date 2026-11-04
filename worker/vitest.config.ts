import { defineConfig, type Plugin } from "vitest/config";

// Tests run in Node. Wrangler turns `import m from "x.wasm"` into a compiled
// WebAssembly.Module; this plugin does the same so tests exercise the real Wasm build.
function compiledWasm(): Plugin {
  return {
    name: "compiled-wasm",
    enforce: "pre",
    load(id) {
      if (!id.endsWith(".wasm")) return null;
      return `import { readFileSync } from "node:fs";\nexport default new WebAssembly.Module(readFileSync(${JSON.stringify(id)}));`;
    },
  };
}

export default defineConfig({
  plugins: [compiledWasm()],
  test: {
    environment: "node",
    include: ["test/**/*.test.ts"],
  },
});
