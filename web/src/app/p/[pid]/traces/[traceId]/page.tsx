"use client";

import { AssistantAction } from "@/components/assistant";
import { post as apiPostA } from "@/lib/api";

import { useParams } from "next/navigation";
import Link from "next/link";
import { useProjectId, useProjectQuery } from "@/lib/hooks";
import { Waterfall } from "@/components/trace/waterfall";
import { ErrorBox, Stat, Card, Table, Th, Td, Badge } from "@/components/ui";
import type { TraceView } from "@/lib/types";
import { fmtMs, fmtTime, fmtUsd } from "@/lib/format";

export default function TracePage() {
  const { traceId } = useParams<{ traceId: string }>();
  const pid = useProjectId();
  const t = useProjectQuery<TraceView>(["trace", traceId], `/traces/${traceId}`);
  if (t.error) return <ErrorBox error={t.error} />;
  if (!t.data) return <div className="text-muted">Loading trace…</div>;
  const d = t.data;
  return (
    <div className="space-y-3">
      <div className="flex items-baseline gap-3">
        <Link href={`/p/${pid}/traces`} className="text-muted hover:text-fg text-xs">← traces</Link>
        <h1 className="text-base font-semibold">{d.root_name}</h1>
        <span className="font-mono text-xs text-muted">{d.trace_id}</span>
        {d.repeated_queries.length > 0 && <Badge tone="warn" className="ml-1">N+1: {d.repeated_queries[0].count}× {d.repeated_queries[0].table || "query"}</Badge>}
        {(() => { const sid = d.spans.map((s) => s.attributes?.["session.id"]).find((x) => typeof x === "string" && x); return sid ? <Link href={`/p/${pid}/browser/sessions/${String(sid)}`} className="ml-2 text-xs text-info hover:underline">browser session →</Link> : null; })()}
        <span className="text-xs text-muted ml-auto">{fmtTime(d.start)}</span>
      </div>
      <Link href={`/p/${pid}/traces/diff?a=${d.trace_id}`} className="rounded-md border px-2 py-1 text-xs hover:bg-panel-2">Compare with…</Link>
      <AssistantAction label="Explain this trace" run={() => apiPostA<{ markdown: string; recording?: { project_id: string; span_id: string; trace_id: string; model: string } }>(`/api/projects/${pid}/assistant/explain-trace`, { trace_id: d.trace_id })} />
      <div className="grid grid-cols-2 gap-3 md:grid-cols-5">
        <Stat label="Duration" value={fmtMs(d.duration_ms)} />
        <Stat label="Spans" value={d.span_count} />
        <Stat label="Errors" value={d.error_count} tone={d.error_count ? "err" : undefined} />
        <Stat label="Services" value={d.services.length} sub={d.services.join(", ")} />
        <Stat label="LLM" value={d.llm_calls} sub={fmtUsd(d.llm_cost_usd)} />
      </div>
      {d.repeated_queries.length > 0 && (
        <Card title={`Repeated queries — ${d.db_calls} queries, ${fmtMs(d.db_ms)} in the database`}>
          <Table><thead><tr><Th>Statement</Th><Th>Called from</Th><Th className="text-right">Times</Th><Th className="text-right">Total</Th></tr></thead>
            <tbody>{d.repeated_queries.map((r, i) => <tr key={i}><Td className="font-mono text-[11px] max-w-[520px] truncate" title={r.statement}>{r.statement}</Td><Td className="font-mono text-[11px]">{r.namespace ? `${r.namespace}.` : ""}{r.function || <span className="text-muted">(unknown)</span>}</Td><Td className="text-right tabular-nums">{r.count}</Td><Td className="text-right font-mono">{fmtMs(r.total_ms)}</Td></tr>)}</tbody></Table>
          <div className="mt-2 text-[11px] text-muted">The same statement from the same function this many times in one request is usually a loop over a queryset: look for a missing <span className="font-mono">select_related</span> / <span className="font-mono">prefetch_related</span> in the function named above.</div>
        </Card>
      )}
      <Waterfall trace={d} />
    </div>
  );
}
