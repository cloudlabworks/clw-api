// Worker entry point: instantiate the receipt-check Wasm module once per isolate and serve the API.

import wasmModule from "./wasm/receipt_check_bg.wasm";
import { check, initSync } from "./wasm/receipt_check.js";
import { createApp } from "./app";

initSync({ module: wasmModule });

export default createApp(check);
