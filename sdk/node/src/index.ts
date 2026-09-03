/**
 * @galileo/node — point your app at Galileo in one call.
 *
 *   import { init } from "@galileo/node";
 *   init({ endpoint: "http://localhost:4318", apiKey: "glk_…", service: "shop-api" });
 *
 * or zero-code: `node --import @galileo/node/register app.js` with GALILEO_ENDPOINT / GALILEO_API_KEY /
 * OTEL_SERVICE_NAME in the environment.
 *
 * What you get on top of OpenTelemetry auto-instrumentation (http, express, fastify, pg, mysql,
 * redis, …): call-site `code.*` attributes on DB/HTTP client spans, `user.id`/`tenant.id` on
 * every span and log after `setIdentity()`, a log bridge for console/pino/winston with trace ids,
 * a `traced()` wrapper for your own functions, and exception capture with `exception.*`.
 */
import { context, trace, SpanStatusCode, type Span, type Attributes } from "@opentelemetry/api";
import { logs, SeverityNumber } from "@opentelemetry/api-logs";
import { NodeSDK } from "@opentelemetry/sdk-node";
import { getNodeAutoInstrumentations } from "@opentelemetry/auto-instrumentations-node";
import { OTLPTraceExporter } from "@opentelemetry/exporter-trace-otlp-http";
import { OTLPLogExporter } from "@opentelemetry/exporter-logs-otlp-http";
import { BatchLogRecordProcessor, LoggerProvider } from "@opentelemetry/sdk-logs";
import { BatchSpanProcessor, type SpanProcessor, type ReadableSpan } from "@opentelemetry/sdk-trace-base";
import { Resource } from "@opentelemetry/resources";
import { callSite } from "./callsite.js";
import { currentIdentity, identityAttributes, setIdentity, runWithIdentity, type Identity } from "./identity.js";

export { setIdentity, runWithIdentity, currentIdentity, callSite };
export type { Identity };

export interface GalileoOptions {
  /** OTLP/HTTP base, e.g. http://localhost:4318 */
  endpoint?: string;
  /** Galileo API key with the ingest scope */
  apiKey?: string;
  service?: string;
  version?: string;
  env?: string;
  /** Attach call-site code.* to DB and HTTP client spans (default true). */
  callSites?: boolean;
  /** Patch console.* to emit log records with trace ids (default true). */
  console?: boolean;
  /** Extra file patterns to skip when walking the stack for the call site. */
  skipFrames?: RegExp[];
  /** Enabled/disabled instrumentations passed to auto-instrumentations-node. */
  instrumentations?: Record<string, { enabled?: boolean }>;
}

let sdk: NodeSDK | undefined;
let loggerProvider: LoggerProvider | undefined;

/** Adds identity + call-site attributes when a span starts; runs in-process, so it is free. */
class GalileoSpanProcessor implements SpanProcessor {
  constructor(private opts: Required<Pick<GalileoOptions, "callSites">> & { skipFrames: RegExp[] }) {}
  onStart(span: Span & Partial<ReadableSpan>): void {
    const attrs: Attributes = identityAttributes(currentIdentity());
    const kind = (span as ReadableSpan).kind;
    const name = (span as ReadableSpan).name ?? "";
    const isClient = kind === 2 /* CLIENT */ || /^(pg|mysql|redis|mongodb|http|fetch)/i.test(name);
    if (this.opts.callSites && isClient) {
      const cs = callSite(process.cwd(), this.opts.skipFrames);
      if (cs) Object.assign(attrs, cs);
    }
    if (Object.keys(attrs).length) span.setAttributes(attrs);
  }
  onEnd(): void {}
  shutdown(): Promise<void> { return Promise.resolve(); }
  forceFlush(): Promise<void> { return Promise.resolve(); }
}

export function init(o: GalileoOptions = {}): void {
  if (sdk) return;
  const endpoint = (o.endpoint ?? process.env.GALILEO_ENDPOINT ?? process.env.OTEL_EXPORTER_OTLP_ENDPOINT ?? "http://localhost:4318").replace(/\/$/, "");
  const apiKey = o.apiKey ?? process.env.GALILEO_API_KEY ?? "";
  const headers: Record<string, string> = apiKey ? { authorization: `Bearer ${apiKey}` } : {};
  const resource = new Resource({
    "service.name": o.service ?? process.env.OTEL_SERVICE_NAME ?? "node-app",
    ...(o.version ?? process.env.GALILEO_RELEASE ? { "service.version": o.version ?? process.env.GALILEO_RELEASE } : {}),
    ...(o.env ?? process.env.GALILEO_ENV ? { "deployment.environment": o.env ?? process.env.GALILEO_ENV } : {}),
    "telemetry.sdk.name": "galileo-node",
  });
  loggerProvider = new LoggerProvider({ resource });
  loggerProvider.addLogRecordProcessor(new BatchLogRecordProcessor(new OTLPLogExporter({ url: `${endpoint}/v1/logs`, headers })));
  logs.setGlobalLoggerProvider(loggerProvider);

  sdk = new NodeSDK({
    resource,
    spanProcessors: [
      new GalileoSpanProcessor({ callSites: o.callSites ?? true, skipFrames: o.skipFrames ?? [] }),
      new BatchSpanProcessor(new OTLPTraceExporter({ url: `${endpoint}/v1/traces`, headers })),
    ],
    instrumentations: [getNodeAutoInstrumentations({
      "@opentelemetry/instrumentation-fs": { enabled: false },
      "@opentelemetry/instrumentation-net": { enabled: false },
      "@opentelemetry/instrumentation-dns": { enabled: false },
      ...(o.instrumentations ?? {}),
    })],
  });
  sdk.start();
  if (o.console ?? true) patchConsole();
  process.on("uncaughtExceptionMonitor", (err) => captureException(err, { "exception.escaped": true }));
  process.on("unhandledRejection", (reason) => captureException(reason instanceof Error ? reason : new Error(String(reason)), { "exception.escaped": true }));
  const stop = () => { void shutdown(); };
  process.once("SIGTERM", stop); process.once("SIGINT", stop);
}

export async function shutdown(): Promise<void> {
  await Promise.allSettled([sdk?.shutdown(), loggerProvider?.shutdown()]);
  sdk = undefined; loggerProvider = undefined;
}

/** Record an error on the active span (or a fresh one) with exception.* so Issues can group it. */
export function captureException(err: unknown, extra: Attributes = {}): void {
  const e = err instanceof Error ? err : new Error(String(err));
  const tracer = trace.getTracer("galileo-node");
  const active = trace.getSpan(context.active());
  const record = (span: Span) => {
    span.recordException(e);
    span.setAttributes({ "exception.type": e.name, "exception.message": e.message, ...(e.stack ? { "exception.stacktrace": e.stack } : {}), ...identityAttributes(currentIdentity()), ...extra });
    span.setStatus({ code: SpanStatusCode.ERROR, message: e.message });
  };
  if (active) record(active);
  else { const s = tracer.startSpan(`exception ${e.name}`); record(s); s.end(); }
  log("error", `${e.name}: ${e.message}`, { "exception.type": e.name, "exception.message": e.message, ...(e.stack ? { "exception.stacktrace": e.stack } : {}) });
}

/** Wrap a function in a span named after it, with args count and identity; rethrows. */
export function traced<T extends (...a: never[]) => unknown>(fn: T, name?: string): T {
  const spanName = name ?? fn.name ?? "fn";
  const wrapped = function (this: unknown, ...args: Parameters<T>) {
    const tracer = trace.getTracer("galileo-node");
    return tracer.startActiveSpan(spanName, (span) => {
      span.setAttributes({ "code.function.name": spanName, ...identityAttributes(currentIdentity()) });
      const done = (ok: boolean, err?: unknown) => { if (!ok) { const e = err instanceof Error ? err : new Error(String(err)); span.recordException(e); span.setStatus({ code: SpanStatusCode.ERROR, message: e.message }); } span.end(); };
      try {
        const out = fn.apply(this, args) as unknown;
        if (out instanceof Promise) return out.then((v) => { done(true); return v; }, (e) => { done(false, e); throw e; });
        done(true); return out;
      } catch (e) { done(false, e); throw e; }
    });
  };
  return wrapped as unknown as T;
}

const SEV: Record<string, SeverityNumber> = { trace: SeverityNumber.TRACE, debug: SeverityNumber.DEBUG, info: SeverityNumber.INFO, warn: SeverityNumber.WARN, error: SeverityNumber.ERROR, fatal: SeverityNumber.FATAL };

/** Emit a log record tied to the active trace and identity. */
export function log(level: keyof typeof SEV, message: string, attributes: Attributes = {}): void {
  logs.getLogger("galileo-node").emit({ severityNumber: SEV[level] ?? SeverityNumber.INFO, severityText: level.toUpperCase(), body: message, attributes: { ...identityAttributes(currentIdentity()), ...attributes }, context: context.active() });
}

function patchConsole(): void {
  const map: [keyof Console, keyof typeof SEV][] = [["debug", "debug"], ["info", "info"], ["log", "info"], ["warn", "warn"], ["error", "error"]];
  for (const [method, level] of map) {
    const orig = console[method] as (...a: unknown[]) => void;
    (console as unknown as Record<string, unknown>)[method] = (...a: unknown[]) => {
      try { log(level, a.map((x) => (typeof x === "string" ? x : x instanceof Error ? `${x.name}: ${x.message}` : JSON.stringify(x))).join(" ")); } catch { /* never break logging */ }
      orig.apply(console, a);
    };
  }
}

/** Pino transport-free bridge: `pino({ hooks: { logMethod: galileoPinoHook } })`. */
export function galileoPinoHook(this: { levels?: { labels: Record<number, string> } }, args: unknown[], method: (...a: unknown[]) => void, level: number): void {
  try { const label = (this.levels?.labels?.[level] ?? "info") as keyof typeof SEV; const [first, ...rest] = args; log(label, typeof first === "string" ? first : rest.find((x) => typeof x === "string") as string ?? JSON.stringify(first), typeof first === "object" && first ? (first as Attributes) : {}); } catch { /* ignore */ }
  method.apply(this, args);
}

/** Express middleware: sets identity from req.user (or a resolver) for everything downstream. */
export function galileoExpress(resolve?: (req: unknown) => Identity | undefined) {
  return (req: { user?: Identity }, _res: unknown, next: () => void) => {
    const id = resolve ? resolve(req) : req.user;
    if (id) runWithIdentity(id, next); else next();
  };
}

// ---------------------------------------------------------------- agent runs
import { AsyncLocalStorage as ALS2 } from "node:async_hooks";
import { randomUUID } from "node:crypto";
const runStore = new ALS2<{ name: string; conversationId: string }>();

/** Group the gateway calls of one conversation into an agent run; `conversationHeaders()` gives the headers to send. */
export function agentRun<T>(name: string, fn: (run: { conversationId: string }) => T, conversationId?: string): T {
  const run = { name, conversationId: conversationId ?? randomUUID().replace(/-/g, "") };
  const tracer = trace.getTracer("galileo-node");
  return runStore.run(run, () => tracer.startActiveSpan(`agent.run ${name}`, (span) => {
    span.setAttributes({ "gen_ai.conversation.id": run.conversationId, "agent.name": name, ...identityAttributes(currentIdentity()) });
    const done = () => span.end();
    try { const out = fn(run) as unknown; if (out instanceof Promise) return out.finally(done) as T; done(); return out as T; } catch (e) { done(); throw e; }
  }));
}
export function conversationHeaders(): Record<string, string> {
  const r = runStore.getStore(); const id = currentIdentity();
  const h: Record<string, string> = {};
  if (r) h["x-galileo-conversation-id"] = r.conversationId;
  if (id?.id !== undefined) h["x-galileo-user-id"] = String(id.id);
  if (id?.tenant !== undefined) h["x-galileo-tenant-id"] = String(id.tenant);
  return h;
}
/** A tool execution inside an agent run. */
export function agentStep<T>(name: string, fn: () => T): T {
  const tracer = trace.getTracer("galileo-node");
  return tracer.startActiveSpan(`agent.step ${name}`, (span) => {
    const r = runStore.getStore();
    span.setAttributes({ "agent.step": name, ...(r ? { "gen_ai.conversation.id": r.conversationId } : {}) });
    const done = () => span.end();
    try { const out = fn() as unknown; if (out instanceof Promise) return out.finally(done) as T; done(); return out as T; } catch (e) { done(); throw e; }
  });
}
