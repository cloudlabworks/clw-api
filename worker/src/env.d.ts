// Wrangler bundles `.wasm` imports as a compiled WebAssembly.Module (CompiledWasm rule).
declare module "*.wasm" {
  const module: WebAssembly.Module;
  export default module;
}
