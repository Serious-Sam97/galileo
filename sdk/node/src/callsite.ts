/**
 * Call-site attribution: which application function issued this DB query / HTTP call.
 * Walks the current stack, skips node internals, node_modules and this SDK, and returns the first
 * frame that belongs to the app, as OTel `code.*` attributes.
 */
export interface CallSite { "code.function.name": string; "code.namespace": string; "code.file.path": string; "code.line.number": number }

const SKIP = [/node:internal/, /[\\/]node_modules[\\/]/, /[\\/]@galileo[\\/]node[\\/]/, /[\\/]dist[\\/]callsite\.js/, /^internal\//];

export function callSite(root = process.cwd(), extraSkip: RegExp[] = []): CallSite | undefined {
  const holder: { stack?: string } = {};
  const prev = Error.stackTraceLimit;
  Error.stackTraceLimit = 40;
  Error.captureStackTrace(holder, callSite);
  Error.stackTraceLimit = prev;
  const lines = (holder.stack ?? "").split("\n").slice(1);
  for (const line of lines) {
    // "    at fn (file:line:col)" or "    at file:line:col"
    const m = /^\s*at\s+(?:(.+?)\s+\()?(?:file:\/\/)?(.+?):(\d+):(\d+)\)?\s*$/.exec(line);
    if (!m) continue;
    const [, fn, file, ln] = m;
    if (SKIP.some((r) => r.test(file)) || extraSkip.some((r) => r.test(file))) continue;
    if (!file.startsWith("/") && !/^[A-Za-z]:\\/.test(file)) continue;
    const rel = file.startsWith(root) ? file.slice(root.length + 1) : file;
    const name = (fn ?? "<anonymous>").replace(/^async\s+/, "");
    const dot = name.lastIndexOf(".");
    return {
      "code.function.name": dot > 0 ? name.slice(dot + 1) : name,
      "code.namespace": dot > 0 ? name.slice(0, dot) : rel.replace(/\.[cm]?[jt]s$/, "").replace(/[\\/]/g, "."),
      "code.file.path": rel,
      "code.line.number": Number(ln),
    };
  }
  return undefined;
}
