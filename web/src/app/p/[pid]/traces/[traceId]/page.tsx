"use client";

import { AssistantAction } from "@/components/assistant";
import { post as apiPostA } from "@/lib/api";

import { useParams } from "next/navigation";
import Link from "next/link";
import { ArrowLeft, GitCompare, Timer } from "lucide-react";
import { useProjectId, useProjectQuery } from "@/lib/hooks";
import { Waterfall } from "@/components/trace/waterfall";
import { ErrorBox, Stat, Card, Table, Th, Td, Badge, Skeleton } from "@/components/ui";
import type { SpanNode, TraceView } from "@/lib/types";
import { fmtMs, fmtTime, fmtUsd } from "@/lib/format";

/**
 * The span that holds most of the request, when one does: not a root, at least half the trace and at
 * least 100 ms. A request with one such span is a different problem from one that is slow everywhere.
 */
function hotSpan(d: TraceView): SpanNode | null {
  const roots = new Set(d.roots);
  const best = d.spans.filter((s) => !roots.has(s.span_id)).sort((a, b) => b.duration_ms - a.duration_ms)[0];
  return best && best.duration_ms >= 100 && best.duration_ms / Math.max(d.duration_ms, 0.001) >= 0.5 ? best : null;
}

function hotHint(s: SpanNode): string {
  const op = String(s.attributes["db.operation.name"] ?? s.attributes["db.operation"] ?? "").toUpperCase();
  if (op === "UPDATE" || op === "DELETE" || op === "INSERT") return "A write this slow on its own is often waiting for a lock another transaction holds.";
  if (op === "SELECT") return "A read this slow on its own usually means a missing index or a large scan.";
  if (s.kind === "client") return "The time is spent waiting on the called service.";
  return "Open it to see its attributes and the code that ran it.";
}

export default function TracePage() {
  const { traceId } = useParams<{ traceId: string }>();
  const pid = useProjectId();
  const t = useProjectQuery<TraceView>(["trace", traceId], `/traces/${traceId}`);
  if (t.error) return <ErrorBox error={t.error} />;
  if (!t.data) return (
    <div className="mx-auto max-w-[1400px] space-y-4">
      <Skeleton className="h-10 w-1/2" /><Skeleton className="h-20" /><Skeleton className="h-96" />
    </div>
  );
  const d = t.data;
  const root = d.spans.find((s) => d.roots.includes(s.span_id));
  const attr = (k: string) => (root?.attributes[k] != null ? String(root.attributes[k]) : null);
  const hot = hotSpan(d);
  const sessionId = d.spans.map((s) => s.attributes?.["session.id"]).find((x) => typeof x === "string" && x);
  const chips = [
    ...d.services,
    attr("http.response.status_code") ?? attr("http.status_code"),
    attr("user.id") && `user ${attr("user.id")}`,
    attr("tenant.id") && `tenant ${attr("tenant.id")}`,
    fmtTime(d.start),
  ].filter(Boolean) as string[];

  return (
    <div className="mx-auto max-w-[1400px] space-y-4">
      <div className="flex flex-wrap items-center gap-2 text-[12.5px]">
        <Link href={`/p/${pid}/traces`} className="inline-flex items-center gap-1 text-muted hover:text-fg"><ArrowLeft size={14} /> Traces</Link>
        <span className="text-faint">/</span>
        <span className="font-mono text-faint">{d.trace_id}</span>
        <div className="ml-auto flex flex-wrap items-center gap-2">
          {sessionId ? <Link href={`/p/${pid}/browser/sessions/${String(sessionId)}`} className="rounded-lg border px-3 py-1.5 hover:bg-panel-2">Browser session</Link> : null}
          <Link href={`/p/${pid}/traces/diff?a=${d.trace_id}`} className="inline-flex items-center gap-1.5 rounded-lg border px-3 py-1.5 hover:bg-panel-2"><GitCompare size={14} /> Compare with…</Link>
        </div>
      </div>

      <div className="flex flex-wrap items-end gap-4">
        <div className="min-w-0">
          <h1 className="truncate font-mono text-2xl font-bold tracking-tight">{d.root_name}</h1>
          <div className="mt-2 flex flex-wrap gap-1.5">
            {chips.map((c) => <span key={c} className="rounded-full border bg-panel/80 px-2.5 py-0.5 text-[12px] text-muted">{c}</span>)}
            {d.repeated_queries.length > 0 && <Badge tone="warn">N+1: {d.repeated_queries[0].count}× {d.repeated_queries[0].table || "query"}</Badge>}
          </div>
        </div>
        <div className="ml-auto text-right">
          <div className={`text-4xl font-bold tabular-nums tracking-tight ${d.duration_ms >= 1000 ? "text-warn" : ""}`}>{fmtMs(d.duration_ms)}</div>
          <div className="text-[12.5px] text-muted">{d.span_count} spans{d.db_calls > 0 ? ` · ${d.db_calls} SQL statements` : ""}</div>
        </div>
      </div>

      {hot && (
        <div className="rise flex items-start gap-3 rounded-2xl border border-warn/35 bg-gradient-to-r from-warn/10 to-transparent px-4 py-3.5">
          <Timer size={18} className="mt-0.5 shrink-0 text-warn drop-shadow-[0_0_6px_var(--warn)]" />
          <div className="space-y-0.5">
            <div className="font-semibold">One span is {Math.round((hot.duration_ms / d.duration_ms) * 1000) / 10}% of this request</div>
            <div className="text-[13px] leading-relaxed text-muted">
              <span className="font-mono text-fg">{hot.name}</span> ran for {fmtMs(hot.duration_ms)}
              {typeof hot.attributes["code.function.name"] === "string" ? <> from <span className="font-mono text-fg">{String(hot.attributes["code.function.name"])}</span></> : null}. {hotHint(hot)}
            </div>
          </div>
        </div>
      )}

      <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
        <Stat label="Errors" value={d.error_count} tone={d.error_count ? "err" : undefined} sub={d.error_count ? "spans failed" : "none failed"} />
        <Stat label="Database" value={d.db_calls ? fmtMs(d.db_ms) : "–"} sub={d.db_calls ? `${d.db_calls} statements` : "no SQL"} />
        <Stat label="Services" value={d.services.length} sub={d.services.join(", ")} />
        <Stat label="LLM" value={d.llm_calls} sub={d.llm_calls ? fmtUsd(d.llm_cost_usd) : "no model calls"} />
      </div>

      <AssistantAction label="Explain this trace" run={() => apiPostA<{ markdown: string; recording?: { project_id: string; span_id: string; trace_id: string; model: string } }>(`/api/projects/${pid}/assistant/explain-trace`, { trace_id: d.trace_id })} />

      {d.repeated_queries.length > 0 && (
        <Card title={`Repeated queries — ${d.db_calls} queries, ${fmtMs(d.db_ms)} in the database`}>
          <Table><thead><tr><Th>Statement</Th><Th>Called from</Th><Th className="text-right">Times</Th><Th className="text-right">Total</Th></tr></thead>
            <tbody>{d.repeated_queries.map((r, i) => <tr key={i}><Td className="font-mono text-[11px] max-w-[520px] truncate" title={r.statement}>{r.statement}</Td><Td className="font-mono text-[11px]">{r.namespace ? `${r.namespace}.` : ""}{r.function || <span className="text-muted">(unknown)</span>}</Td><Td className="text-right tabular-nums">{r.count}</Td><Td className="text-right font-mono">{fmtMs(r.total_ms)}</Td></tr>)}</tbody></Table>
          <div className="mt-2 text-[11.5px] text-muted">The same statement from the same function this many times in one request is usually a loop issuing one query per item: load them in one query before the loop (in Django, <span className="font-mono">select_related</span> / <span className="font-mono">prefetch_related</span>).</div>
        </Card>
      )}
      <Waterfall trace={d} hot={hot?.span_id} />
    </div>
  );
}
