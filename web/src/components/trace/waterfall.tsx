"use client";

import { useMemo, useState } from "react";
import clsx from "clsx";
import { ChevronDown, ChevronRight, Layers } from "lucide-react";
import type { Caller, SpanNode, TraceView } from "@/lib/types";
import { fmtMs, fmtUsd, colorFor, fmtNum } from "@/lib/format";
import { Badge, Drawer, Table, Th, Td, Empty } from "@/components/ui";
import { useProjectQuery } from "@/lib/hooks";
import { C } from "@/lib/palette";

/** Siblings with the same name are shown once with a ×N badge when there are at least this many. */
const COLLAPSE_AT = 5;

export function Waterfall({ trace, hot }: { trace: TraceView; hot?: string }) {
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [selected, setSelected] = useState<SpanNode | null>(null);
  const services = useMemo(() => Object.fromEntries(trace.services.map((s, i) => [s, colorFor(i)])), [trace.services]);
  const byId = useMemo(() => new Map(trace.spans.map((s) => [s.span_id, s])), [trace.spans]);

  const [expandedGroups, setExpandedGroups] = useState<Set<string>>(new Set());
  // Rows plus, for the first span of a large group of same-named siblings, how many it stands for.
  const rows = useMemo(() => {
    const out: { s: SpanNode; groupKey?: string; groupSize?: number; groupMs?: number; hiddenInGroup?: boolean }[] = [];
    const visit = (id: string) => {
      const s = byId.get(id);
      if (!s) return;
      out.push({ s });
      if (collapsed.has(id)) return;
      const kids = s.children.map((c) => byId.get(c)).filter((k): k is SpanNode => !!k);
      const counts = new Map<string, SpanNode[]>();
      kids.forEach((k) => counts.set(k.name, [...(counts.get(k.name) ?? []), k]));
      const seen = new Set<string>();
      for (const k of kids) {
        const group = counts.get(k.name)!;
        const key = `${id}:${k.name}`;
        if (group.length >= COLLAPSE_AT && !expandedGroups.has(key)) {
          if (seen.has(k.name)) continue;
          seen.add(k.name);
          out.push({ s: k, groupKey: key, groupSize: group.length, groupMs: group.reduce((a, g) => a + g.duration_ms, 0) });
          continue;
        }
        if (group.length >= COLLAPSE_AT && !seen.has(k.name)) {
          seen.add(k.name);
          out.push({ s: k, groupKey: key, groupSize: group.length, groupMs: group.reduce((a, g) => a + g.duration_ms, 0), hiddenInGroup: false });
          if (!collapsed.has(k.span_id)) k.children.forEach(visit);
          continue;
        }
        visit(k.span_id);
      }
    };
    trace.roots.forEach(visit);
    return out;
  }, [trace.roots, byId, collapsed, expandedGroups]);

  const total = Math.max(trace.duration_ms, 0.001);
  const ticks = [0, 0.25, 0.5, 0.75, 1];

  return (
    <div className="rise overflow-hidden rounded-2xl border bg-panel/90">
      <div className="flex flex-wrap items-center gap-3 border-b px-4 py-2.5 text-xs text-muted">
        {trace.services.map((s) => <span key={s} className="flex items-center gap-1.5"><span className="h-2.5 w-2.5 rounded-[3px]" style={{ background: services[s], boxShadow: `0 0 8px ${services[s]}` }} />{s}</span>)}
        <span className="ml-auto">{trace.span_count} spans · {fmtMs(trace.duration_ms)}{trace.db_calls > 0 && <> · {trace.db_calls} queries ({fmtMs(trace.db_ms)})</>}{trace.error_count > 0 && <> · <span className="text-err">{trace.error_count} errors</span></>}{trace.llm_calls > 0 && <> · {trace.llm_calls} LLM calls ({fmtUsd(trace.llm_cost_usd)})</>}</span>
      </div>
      <div className="grid text-[12px]" style={{ gridTemplateColumns: "minmax(280px, 34%) 1fr" }}>
        <div className="border-b px-3 py-1.5 text-[10.5px] font-semibold uppercase tracking-[0.06em] text-faint">Span</div>
        <div className="relative border-b h-7 mr-14">
          {ticks.map((t) => <span key={t} className="absolute top-1.5 text-[10px] text-faint" style={{ left: `${t * 100}%`, transform: t === 0 ? "none" : t === 1 ? "translateX(-100%)" : "translateX(-50%)" }}>{fmtMs(total * t)}</span>)}
        </div>
        {rows.map(({ s, groupKey, groupSize, groupMs }) => {
          const color = services[s.service_name] ?? C.faint;
          const hasKids = s.children.length > 0;
          const isErr = s.status.code === "error";
          const isLlm = "gen_ai.system" in s.attributes || "gen_ai.request.model" in s.attributes;
          const sql = typeof s.attributes["db.statement"] === "string" ? String(s.attributes["db.statement"]) : undefined;
          const fn = s.attributes["code.function.name"];
          const isGroupHead = groupKey && !expandedGroups.has(groupKey);
          return (
            <div key={s.span_id} className="contents group">
              <div className={clsx("flex min-h-[28px] items-center gap-1 border-b border-border/40 px-1 py-0.5 cursor-pointer group-hover:bg-panel-2/70", selected?.span_id === s.span_id && "bg-accent/10", hot === s.span_id && "bg-warn/[0.07]")} style={{ paddingLeft: 6 + s.depth * 14 }} onClick={() => setSelected(s)} title={sql ?? s.name}>
                <button className="w-4 shrink-0 text-muted" onClick={(e) => { e.stopPropagation(); if (!hasKids) return; const n = new Set(collapsed); if (n.has(s.span_id)) n.delete(s.span_id); else n.add(s.span_id); setCollapsed(n); }}>
                  {hasKids ? (collapsed.has(s.span_id) ? <ChevronRight size={12} /> : <ChevronDown size={12} />) : null}
                </button>
                <span className="h-2 w-2 shrink-0 rounded-[3px]" style={{ background: hot === s.span_id ? C.warn : color }} />
                <span className={clsx("truncate", isErr && "text-err")}>{s.name}</span>
                {groupKey && groupSize && (
                  <button className={clsx("ml-1 inline-flex items-center gap-0.5 rounded px-1 text-[10px]", isGroupHead ? "bg-warn/20 text-warn" : "bg-panel-2 text-muted")} title={isGroupHead ? `${groupSize} identical siblings, ${fmtMs(groupMs ?? 0)} total. Click to expand.` : "Collapse"} onClick={(e) => { e.stopPropagation(); const n = new Set(expandedGroups); if (n.has(groupKey)) n.delete(groupKey); else n.add(groupKey); setExpandedGroups(n); }}>
                    <Layers size={10} /> ×{groupSize}
                  </button>
                )}
                {typeof fn === "string" && <span className="ml-1 truncate font-mono text-[10px] text-muted">{String(fn)}()</span>}
                {isLlm && <Badge tone="accent" className="ml-1">LLM</Badge>}
                {s.orphan && <Badge tone="warn" className="ml-1">orphan</Badge>}
                <span className="ml-auto shrink-0 text-[10px] text-muted">{s.service_name}</span>
              </div>
              <div className={clsx("relative border-b border-border/40 py-0.5 cursor-pointer group-hover:bg-panel-2/70", hot === s.span_id && "bg-warn/[0.07]")} onClick={() => setSelected(s)} style={{ backgroundImage: "repeating-linear-gradient(90deg, transparent 0, transparent calc(25% - 1px), rgb(42 31 71 / 0.6) calc(25% - 1px), rgb(42 31 71 / 0.6) 25%)" }}>
                <div className={clsx("absolute top-[7px] h-3.5 rounded-[4px]", isErr && "ring-1 ring-err")} style={{ left: `${(s.offset_ms / total) * 100}%`, width: `max(3px, ${(s.duration_ms / total) * 100}%)`, background: hot === s.span_id ? `linear-gradient(90deg, ${C.warn}, ${C.accent})` : color, opacity: hot === s.span_id ? 1 : 0.85, boxShadow: hot === s.span_id ? "0 0 14px rgb(255 92 207 / 0.5)" : undefined }} />
                <span className={clsx("absolute top-[6px] text-[10.5px] whitespace-nowrap", hot === s.span_id ? "text-warn font-semibold" : "text-muted")} style={{ left: `calc(${Math.min((s.offset_ms + s.duration_ms) / total, 0.88) * 100}% + 6px)` }}>{fmtMs(s.duration_ms)}</span>
              </div>
            </div>
          );
        })}
      </div>
      <Drawer open={!!selected} onClose={() => setSelected(null)} title={selected?.name}>
        {selected && <SpanDetails span={selected} />}
      </Drawer>
    </div>
  );
}

function KV({ obj }: { obj: Record<string, unknown> }) {
  const entries = Object.entries(obj);
  if (!entries.length) return <div className="text-muted text-xs">none</div>;
  return (
    <div className="grid gap-x-3 gap-y-1 text-[12px]" style={{ gridTemplateColumns: "max-content 1fr" }}>
      {entries.map(([k, v]) => (
        <div key={k} className="contents">
          <div className="font-mono text-muted truncate">{k}</div>
          <div className="font-mono break-all whitespace-pre-wrap">{typeof v === "string" ? v : JSON.stringify(v)}</div>
        </div>
      ))}
    </div>
  );
}

const CODE_KEYS = ["code.function.name", "code.namespace", "code.file.path", "code.line.number", "code.args", "code.return_type", "django.view.class", "django.view.name", "django.view.args"];

function CodeSection({ span }: { span: SpanNode }) {
  const a = span.attributes;
  const fn = a["code.function.name"];
  if (typeof fn !== "string") return null;
  const isLibrary = "db.system" in a || "http.url" in a || "url.full" in a || "gen_ai.system" in a;
  const where = `${a["code.namespace"] ? `${a["code.namespace"]}.` : ""}${fn}`;
  const file = a["code.file.path"] ? `${a["code.file.path"]}${a["code.line.number"] ? `:${a["code.line.number"]}` : ""}` : null;
  const args = Object.entries(a).filter(([k]) => k.startsWith("code.arg."));
  return (
    <Section title="Code">
      <div className="rounded-lg border bg-bg/70 p-2.5 text-[12px] space-y-1">
        <div>{isLibrary ? <span className="text-muted">called from </span> : null}<span className="font-mono text-accent">{where}()</span></div>
        {file && <div className="font-mono text-muted text-[11px]">{file}</div>}
        {typeof a["code.args"] === "string" && <div className="text-[11px]"><span className="text-muted">args </span><span className="font-mono">{String(a["code.args"])}</span></div>}
        {args.map(([k, v]) => <div key={k} className="text-[11px] font-mono"><span className="text-muted">{k.replace("code.arg.", "")} = </span>{String(v)}</div>)}
        {typeof a["django.view.class"] === "string" && <div className="text-[11px] text-muted">view {String(a["django.view.class"])}{a["django.view.args"] ? ` (${String(a["django.view.args"])})` : ""}</div>}
      </div>
    </Section>
  );
}

function CallersTab({ span }: { span: SpanNode }) {
  const statement = String(span.attributes["db.statement"] ?? "");
  const table = String(span.attributes["db.table"] ?? "");
  const q = useProjectQuery<{ callers: Caller[] }>(["callers", statement, table], `/db/callers?statement=${encodeURIComponent(statement)}&table=${encodeURIComponent(table)}&last_seconds=86400`, { enabled: !!statement });
  if (!statement) return null;
  return (
    <Section title="Callers of this statement (24h)">
      {q.data?.callers.length ? (
        <Table><thead><tr><Th>Function</Th><Th className="text-right">Calls</Th><Th className="text-right">Traces</Th><Th className="text-right">p50</Th><Th className="text-right">p95</Th></tr></thead>
          <tbody>{q.data.callers.map((c, i) => <tr key={i}><Td><div className="font-mono text-[11px]">{c.namespace ? `${c.namespace}.` : ""}{c.function || "(unknown)"}</div><div className="text-[10px] text-muted font-mono">{c.file}{c.line ? `:${c.line}` : ""}</div></Td><Td className="text-right">{fmtNum(c.calls)}</Td><Td className="text-right">{fmtNum(c.traces)}</Td><Td className="text-right font-mono">{fmtMs(c.p50_ms)}</Td><Td className="text-right font-mono">{fmtMs(c.p95_ms)}</Td></tr>)}</tbody></Table>
      ) : q.isLoading ? <div className="text-muted text-xs">Loading…</div> : <Empty>No other callers recorded.</Empty>}
    </Section>
  );
}

export function SpanDetails({ span }: { span: SpanNode }) {
  const llmPrompt = span.attributes["gen_ai.prompt"];
  const llmCompletion = span.attributes["gen_ai.completion"];
  const sql = span.attributes["db.statement"];
  const rest = Object.fromEntries(Object.entries(span.attributes).filter(([k]) => k !== "gen_ai.prompt" && k !== "gen_ai.completion" && k !== "db.statement" && k !== "db.query.text" && !CODE_KEYS.includes(k) && !k.startsWith("code.arg.")));
  return (
    <div className="space-y-4 text-sm">
      <div className="flex flex-wrap gap-2">
        <Badge tone={span.status.code === "error" ? "err" : span.status.code === "ok" ? "ok" : "muted"}>{span.status.code}{span.status.message ? `: ${span.status.message}` : ""}</Badge>
        <Badge>{span.kind}</Badge><Badge>{span.service_name}</Badge><Badge>{fmtMs(span.duration_ms)}</Badge>
        {typeof span.attributes["db.row_count"] === "number" && <Badge>{String(span.attributes["db.row_count"])} rows</Badge>}
      </div>
      <div className="text-[11px] text-muted font-mono">trace {span.trace_id} · span {span.span_id}{span.parent_span_id ? ` · parent ${span.parent_span_id}` : ""}</div>
      <CodeSection span={span} />
      {typeof sql === "string" && <Section title="SQL"><pre className="whitespace-pre-wrap rounded-lg border bg-bg/70 p-2.5 font-mono text-[11px] max-h-48 overflow-auto scroll-thin">{sql}</pre>{typeof span.attributes["db.params"] === "string" && <div className="mt-1 font-mono text-[11px] text-muted">params {String(span.attributes["db.params"])}</div>}</Section>}
      {typeof sql === "string" && <CallersTab span={span} />}
      {typeof llmPrompt === "string" && <Section title="Prompt"><pre className="whitespace-pre-wrap rounded-lg border bg-bg/70 p-2.5 font-mono text-[11px] max-h-64 overflow-auto scroll-thin">{llmPrompt}</pre></Section>}
      {typeof llmCompletion === "string" && <Section title="Completion"><pre className="whitespace-pre-wrap rounded-lg border bg-bg/70 p-2.5 font-mono text-[11px] max-h-64 overflow-auto scroll-thin">{llmCompletion}</pre></Section>}
      <Section title="Attributes"><KV obj={rest} /></Section>
      <Section title="Resource"><KV obj={span.resource} /></Section>
      {span.events.length > 0 && (
        <Section title="Events">
          {span.events.map((e, i) => (
            <div key={i} className="mb-2 rounded-lg border bg-bg/70 p-2.5">
              <div className="text-xs font-medium">{e.name} <span className="text-muted font-normal">{new Date(e.timestamp).toLocaleTimeString()}</span></div>
              <KV obj={e.attributes} />
            </div>
          ))}
        </Section>
      )}
      {span.links.length > 0 && <Section title="Links">{span.links.map((l, i) => <div key={i} className="font-mono text-xs">{l.trace_id} / {l.span_id}</div>)}</Section>}
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return <div><div className="mb-1.5 text-[11px] font-semibold uppercase tracking-[0.06em] text-faint">{title}</div>{children}</div>;
}
