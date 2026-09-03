import { diag, DiagConsoleLogger, DiagLogLevel, trace } from "@opentelemetry/api";
diag.setLogger(new DiagConsoleLogger(), DiagLogLevel.DEBUG);
const { init, shutdown, traced } = await import("../dist/index.js");
init({ service: "node-smoke" });
const work = traced(async function work() { await new Promise((r) => setTimeout(r, 5)); return 1; });
await work();
const s = trace.getTracer("t").startSpan("manual"); s.end();
await shutdown();
console.log("smoke done");
