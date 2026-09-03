"use client";

import { use } from "react";
import Link from "next/link";
import clsx from "clsx";
import { useProjectId, useProjectQuery } from "@/lib/hooks";
import { Card, Stat, Empty, ErrorBox, Badge } from "@/components/ui";
import { fmtMs, fmtTime, fmtNum, fmtUsd } from "@/lib/format";
import { AssistantAction } from "@/components/assistant";
import { post as apiPost } from "@/lib/api";
import type { AgentTurn } from "@/lib/types";

export default function AgentRunPage({ params }: { params: Promise<{ cid: string }> }) {
  const { cid } = use(params);
  const pid = useProjectId();
  const q = useProjectQuery<{ conversation_id: string; user_id: string; totals: { turns: number; cost_usd: number; input_tokens: number; output_tokens: number; errors: number; tool_calls: number }; turns: AgentTurn[] }>(["agent-run", cid], `/agent-runs/${cid}`);
  const d = q.data;
  const first = d?.turns[0];
  const stalls = (d?.turns ?? []).map((t, i, arr) => i > 0 ? (new Date(t.timestamp).getTime() - new Date(arr[i - 1].timestamp).getTime() - arr[i - 1].duration_ms) : 0);
  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2 flex-wrap">
        <Link href={`/p/${pid}/ai`} className="text-muted hover:text-fg text-sm">← AI</Link>
        <h1 className="text-base font-semibold font-mono">run {cid.slice(0, 12)}…</h1>
        {d?.user_id && <Link href={`/p/${pid}/users/${encodeURIComponent(d.user_id)}`} className="text-info hover:underline text-sm">user {d.user_id}</Link>}
        {first && <span className="text-muted text-xs">{fmtTime(first.timestamp)} · {first.route} · {first.model}</span>}
      </div>
      <ErrorBox error={q.error} />
      {d && (
        <div className="grid grid-cols-2 gap-3 md:grid-cols-6">
          <Stat label="Turns" value={d.totals.turns} sub={`${d.totals.tool_calls} with tool calls`} />
          <Stat label="Cost" value={fmtUsd(d.totals.cost_usd)} />
          <Stat label="Tokens" value={fmtNum(d.totals.input_tokens + d.totals.output_tokens, 0)} sub={`${fmtNum(d.totals.input_tokens, 0)} in / ${fmtNum(d.totals.output_tokens, 0)} out`} />
          <Stat label="Model time" value={fmtMs(d.turns.reduce((a, t) => a + t.duration_ms, 0))} />
          <Stat label="Between turns" value={fmtMs(stalls.reduce((a, b) => a + Math.max(0, b), 0))} sub="app + tools + user" />
          <Stat label="Errors" value={d.totals.errors} tone={d.totals.errors ? "err" : undefined} />
        </div>
      )}
      {d && d.turns.length > 0 && <AssistantAction label="Explain this run" run={() => apiPost<{ answer: string; recording?: { project_id: string; span_id: string; trace_id: string; model: string } }>(`/api/projects/${pid}/assistant/chat`, { messages: [{ role: "user", content: `Explain agent run ${cid}: summarize what the user asked, which tools were called, where time and cost went, and any failures. Use get_trace on trace ${first?.trace_id} and the run's turns.` }], context: { page: "agents", last_seconds: 7 * 86400 } }).then((r) => ({ markdown: r.answer, recording: r.recording }))} />}
      {d && d.turns.length === 0 && <Empty>No turns found for this conversation.</Empty>}
      <ol className="space-y-2">
        {d?.turns.map((t, i) => {
          const tools = Array.isArray(t.tool_calls) ? (t.tool_calls as { name?: string; function?: { name?: string; arguments?: unknown }; arguments?: unknown }[]) : [];
          const stall = stalls[i];
          return (
            <li key={t.span_id}>
              {stall > 3000 && <div className="ml-6 my-1 text-[11px] text-warn">⏸ {fmtMs(stall)} between turns</div>}
              <Card title={<span className="flex items-center gap-2 flex-wrap"><Badge tone={t.status_code === "error" ? "err" : "accent"}>turn {i + 1}</Badge><span className="text-xs text-muted font-normal">{fmtTime(t.timestamp)} · {fmtMs(t.duration_ms)}{t.ttft_ms ? ` · TTFT ${fmtMs(t.ttft_ms)}` : ""} · {fmtNum(t.input_tokens, 0)}/{fmtNum(t.output_tokens, 0)} tok · {fmtUsd(t.cost_usd)}</span>{t.cache_hit === "true" && <Badge tone="accent">cache</Badge>}{t.guardrail && <Badge tone="warn">{t.guardrail}</Badge>}{t.fallback_index > 0 && <Badge tone="warn">fallback {t.fallback_index}</Badge>}<Link href={`/p/${pid}/traces/${t.trace_id}`} className="ml-auto text-xs text-info hover:underline font-normal">trace →</Link></span>}>
                <div className="grid gap-2 md:grid-cols-2 text-[12px]">
                  <div><div className="text-[10px] uppercase text-muted mb-1">prompt (last turn)</div><pre className="whitespace-pre-wrap rounded border bg-bg p-2 font-mono text-[11px] max-h-40 overflow-auto scroll-thin">{t.prompt ? t.prompt.split("\n[").slice(-1)[0].replace(/^\[?user\] /, "") : "(not recorded)"}</pre></div>
                  <div><div className="text-[10px] uppercase text-muted mb-1">completion · {t.finish_reason}</div><pre className="whitespace-pre-wrap rounded border bg-bg p-2 font-mono text-[11px] max-h-40 overflow-auto scroll-thin">{t.completion || (tools.length ? "(tool calls)" : "(not recorded)")}</pre></div>
                </div>
                {tools.length > 0 && <div className="mt-2 flex flex-wrap gap-1">{tools.map((tc, j) => { const name = tc.function?.name ?? tc.name ?? "tool"; const args = tc.function?.arguments ?? tc.arguments; return <span key={j} className="rounded border bg-panel px-1.5 py-0.5 font-mono text-[10px]" title={typeof args === "string" ? args : JSON.stringify(args)}>🔧 {name}</span>; })}</div>}
                {t.app_spans.length > 0 && (
                  <div className="mt-2 space-y-0.5">{t.app_spans.slice(0, 30).map((s) => <div key={s.span_id} className={clsx("flex items-center gap-2 text-[11px] border-l-2 pl-2", s.status_code === "error" ? "border-err" : "border-accent/40")}><span className="text-muted">↳ {s.service}</span><span className="font-mono">{s.name}</span>{s.function && <span className="text-muted">{s.function}</span>}{s.table && <span className="text-muted">{s.table}</span>}<span className="text-muted">{fmtMs(s.duration_ms)}</span>{s.exception && <Badge tone="err">{s.exception}</Badge>}</div>)}{t.app_spans.length > 30 && <div className="text-[10px] text-muted">…{t.app_spans.length - 30} more spans</div>}</div>
                )}
              </Card>
            </li>
          );
        })}
      </ol>
    </div>
  );
}
