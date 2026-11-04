// Vite resolves `?raw` imports to the file contents (used by tests to read openapi.yaml).
declare module "*?raw" {
  const text: string;
  export default text;
}
