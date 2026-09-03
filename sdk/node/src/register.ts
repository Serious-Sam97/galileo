// `node --import @galileo/node/register app.js` — zero-code setup from the environment.
// Registers the OpenTelemetry ESM loader hook first so `import express from "express"` (and pg,
// mysql, …) get patched; CommonJS requires are patched without it.
import { register } from "node:module";
import { init } from "./index.js";

try {
  register("@opentelemetry/instrumentation/hook.mjs", import.meta.url);
} catch {
  // older Node without module.register: CommonJS apps still work
}
init();
