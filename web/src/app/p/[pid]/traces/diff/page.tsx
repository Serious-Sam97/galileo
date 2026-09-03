"use client";

import React, { useEffect, useMemo, useState } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useQuery } from "@tanstack/react-query";
import clsx from "clsx";
import { get, post } from "@/lib/api";
import { useProjectId } from "@/lib/hooks";
import { Card, Input, Button, Badge, Empty } from "@/components/ui";
import { fmtMs, fmtTime } from "@/lib/format";
import type { TraceView, SpanNode } from "@/lib/types";

type Row = { key: string; a?: SpanNode; b?: SpanNode; depth: number };

/** Align spans of two traces by (service, name) in document order; unmatched spans stay on their side. */
function align(a: SpanNode[], b: SpanNode[]): Row[] {
  const keyOf = (s: SpanNode) => `${s.service_name}|${s.name}`;
  const rows: Row[] = [];
  const usedB = new Set<number>();
  let cursor = 0;
  for (const sa of a) {
    const k = keyOf(sa);
    let found = -1;
    for (let j = cursor; j < b.length; j++) { if (!usedB.has(j) && keyOf(b[j]) === k) { found = j; break; } }
    if (found < 0) { for (let j = 0; j < cursor; j++) { if (!usedB.has(j) && keyOf(b[j]) === k) { found = j; break; } } }
    if (found >= 0) { usedB.add(found); cursor = found + 1; rows.push({ key: k, a: sa, b: b[found], depth: sa.depth }); }
    else rows.push({ key: k, a: sa, depth: sa.depth });
  }
  b.forEach((sb, j) => { if (!usedB.has(j)) rows.push({ key: keyOf(sb), b: sb, depth: sb.depth }); });
  return rows;
}

/** Plain fetch state for a trace; big traces (tens of thousands of spans) are kept but the table is capped. */
function useTrace(pid: string, id: string) {
  const [state, set] = useState<{ id: string; data?: TraceView; error?: string; loading: boolean }>({ id: "", loading: false });
  useEffect(() => {
    if (!/^[0-9a-f]{32}$/i.test(id)) { set({ id, loading: false }); return; }
    let alive = true;
    set({ id, loading: true });
    get<TraceView>(`/api/projects/${pid}/traces/${id}`).then((d) => { if (alive) set({ id, data: d, loading: false }); }).catch((e) => { if (alive) set({ id, error: String(e), loading: false }); });
    return () => { alive = false; };
  }, [pid, id]);
  return { data: state.id === id ? state.data : undefined, error: state.id === id ? state.error : undefined, isFetching: state.id === id && state.loading };
}

function isDb(s?: SpanNode) { return !!s && (!!s.attributes?.["db.system"] || !!s.attributes?.["db.statement"]); }

function DiffInner() {
  const pid = useProjectId();
  const router = useRouter();
  const [ids, setIds] = useState<{ a: string; b: string }>({ a: "", b: "" });
  useEffect(() => {
    const read = () => { const p = new URLSearchParams(window.location.search); setIds({ a: p.get("a") ?? "", b: p.get("b") ?? "" }); };
    read();
    window.addEventListener("popstate", read);
    return () => window.removeEventListener("popstate", read);
  }, []);
  const { a, b } = ids;
  const [bInput, setBInput] = useState("");
  const ta = useTrace(pid, a);
  const tb = useTrace(pid, b);
  // candidates: recent traces with the same root name (fast and slow), for picking B
  const root = ta.data?.root_name;
  const cands = useQuery({
    queryKey: ["diff-cands", pid, root],
    enabled: !!root && !b,
    queryFn: () => post<{ raw: { columns: string[]; rows: unknown[][] } }>(`/api/projects/${pid}/query`, { dataset: "spans", time_range: { last_seconds: 86400 }, calculations: [], filters: [{ field: "name", op: "eq", value: root }, { field: "parent_span_id", op: "eq", value: "" }], orders: [{ field: "duration_ms", direction: "desc" }], limit: 12, columns: ["trace_id", "duration_ms", "timestamp", "status_code", "user_id"] }),
  });
  const rows = useMemo(() => (ta.data?.spans && tb.data?.spans ? align(ta.data.spans, tb.data.spans) : []), [ta.data, tb.data]);
  const summary = useMemo(() => {
    if (!ta.data || !tb.data) return null;
    const onlyA = rows.filter((r) => r.a && !r.b).length, onlyB = rows.filter((r) => r.b && !r.a).length;
    const dbA = (ta.data.spans ?? []).filter((s) => isDb(s)).length, dbB = (tb.data.spans ?? []).filter((s) => isDb(s)).length;
    const errA = ta.data.error_count, errB = tb.data.error_count;
    const biggest = rows.filter((r) => r.a && r.b).map((r) => ({ r, d: r.b!.duration_ms - r.a!.duration_ms })).sort((x, y) => Math.abs(y.d) - Math.abs(x.d)).slice(0, 3);
    return { delta: tb.data.duration_ms - ta.data.duration_ms, onlyA, onlyB, dbA, dbB, errA, errB, biggest };
  }, [rows, ta.data, tb.data]);

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <h1 className="text-base font-semibold">Trace diff</h1>
        <span className="text-xs text-muted">A = baseline, B = compared. Spans are aligned by service and name; the delta is B − A.</span>
      </div>
      <div className="grid gap-3 md:grid-cols-2">
        <Card title="A">
          {ta.data ? <TraceHead t={ta.data} pid={pid} /> : <div className="text-sm text-muted">{ta.error ? ta.error : a ? "loading…" : "missing ?a=<trace id>"}</div>}
        </Card>
        <Card title="B">
          {tb.data ? <TraceHead t={tb.data} pid={pid} /> : tb.isFetching ? <div className="text-sm text-muted">loading {b}…</div> : tb.error ? <div className="text-sm text-err">{tb.error}</div> : (
            <div className="space-y-2">
              <div className="flex gap-2"><Input className="font-mono flex-1" placeholder="trace id" value={bInput} onChange={(e) => setBInput(e.target.value)} /><Button variant="primary" onClick={() => setIds({ a, b: bInput.trim() })} disabled={!/^[0-9a-f]{32}$/i.test(bInput.trim())}>Compare</Button></div>
              {root && <div className="text-xs text-muted">Recent traces of <span className="font-mono">{root}</span> (slowest first):</div>}
              {cands.data?.raw?.rows && (() => { const c = cands.data.raw.columns; const ix = (n: string) => c.indexOf(n); return (
                <div className="max-h-60 overflow-auto rounded border text-xs">
                  {cands.data.raw.rows.filter((r) => r[ix("trace_id")] !== a).map((r, i) => (
                    <button key={i} onClick={() => { const nb = String(r[ix("trace_id")]); window.history.pushState(null, "", `/p/${pid}/traces/diff?a=${a}&b=${nb}`); setIds({ a, b: nb }); }} className="flex w-full items-center gap-3 px-2 py-1 text-left hover:bg-panel-2">
                      <span className="text-muted">{fmtTime(String(r[ix("timestamp")]))}</span>
                      <span className="font-mono">{fmtMs(Number(r[ix("duration_ms")]))}</span>
                      <Badge tone={String(r[ix("status_code")]) === "error" ? "err" : "muted"}>{String(r[ix("status_code")])}</Badge>
                      <span className="truncate text-muted">{String(r[ix("user_id")] ?? "")}</span>
                    </button>
                  ))}
                </div>
              ); })()}
            </div>
          )}
        </Card>
      </div>
      {summary && (
        <Card title="Summary">
          <div className="flex flex-wrap gap-4 text-sm">
            <div>Total <b className={clsx(summary.delta > 0 ? "text-err" : "text-ok")}>{summary.delta > 0 ? "+" : ""}{fmtMs(summary.delta)}</b></div>
            <div>DB calls <b>{summary.dbA} → {summary.dbB}</b>{summary.dbB > summary.dbA && <span className="text-warn"> (+{summary.dbB - summary.dbA})</span>}</div>
            <div>Errors <b>{summary.errA} → {summary.errB}</b></div>
            <div>Only in A <b>{summary.onlyA}</b> · only in B <b>{summary.onlyB}</b></div>
          </div>
          {summary.biggest.length > 0 && <div className="mt-2 text-xs text-muted">Biggest changes: {summary.biggest.map(({ r, d }) => <span key={r.key} className="mr-3 font-mono">{r.a!.name} {d > 0 ? "+" : ""}{fmtMs(d)}</span>)}</div>}
        </Card>
      )}
      {rows.length > 0 ? (
        <div className="table-wrap rounded-md border">
          <table className="w-full text-left text-[12px]">
            <thead><tr className="border-b text-muted"><th className="px-2 py-1 font-medium">Span</th><th className="px-2 py-1 text-right font-medium">A</th><th className="px-2 py-1 text-right font-medium">B</th><th className="px-2 py-1 text-right font-medium">Δ</th><th className="px-2 py-1 font-medium">Status</th></tr></thead>
            <tbody>
              {rows.slice(0, 800).map((r, i) => {
                const d = r.a && r.b ? r.b.duration_ms - r.a.duration_ms : null;
                const s = r.a ?? r.b!;
                return (
                  <tr key={i} className={clsx("border-b", !r.a && "bg-ok/10", !r.b && "bg-err/10")}>
                    <td className="px-2 py-1 whitespace-nowrap" style={{ paddingLeft: 8 + r.depth * 12 }}><span className="text-muted">{s.service_name}</span> <span className={clsx(isDb(s) && "font-mono")}>{s.name}</span>{!r.a && <Badge tone="ok" className="ml-2">only B</Badge>}{!r.b && <Badge tone="err" className="ml-2">only A</Badge>}</td>
                    <td className="px-2 py-1 text-right font-mono">{r.a ? fmtMs(r.a.duration_ms) : "–"}</td>
                    <td className="px-2 py-1 text-right font-mono">{r.b ? fmtMs(r.b.duration_ms) : "–"}</td>
                    <td className={clsx("px-2 py-1 text-right font-mono", d !== null && Math.abs(d) >= 1 && (d > 0 ? "text-err" : "text-ok"))}>{d !== null ? `${d > 0 ? "+" : ""}${fmtMs(d)}` : ""}</td>
                    <td className="px-2 py-1">{(r.a?.status.code === "error" || r.b?.status.code === "error") && <Badge tone="err">{r.a?.status.code === "error" ? "A" : ""}{r.b?.status.code === "error" ? "B" : ""} error</Badge>}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
          {rows.length > 800 && <div className="px-2 py-1 text-xs text-muted">showing the first 800 of {rows.length} aligned spans</div>}
        </div>
      ) : (ta.data && !b && <Empty>Pick a second trace above to compare.</Empty>)}
    </div>
  );
}

function TraceHead({ t, pid }: { t: TraceView; pid: string }) {
  return (
    <div className="text-sm">
      <Link href={`/p/${pid}/traces/${t.trace_id}`} className="font-mono text-xs text-accent hover:underline">{t.trace_id}</Link>
      <div className="mt-1 flex flex-wrap gap-3"><span>{t.root_name}</span><span className="font-mono">{fmtMs(t.duration_ms)}</span><span className="text-muted">{t.span_count} spans · {t.db_calls} db · {t.error_count} errors</span><span className="text-muted">{fmtTime(t.start)}</span></div>
    </div>
  );
}

class Boundary extends React.Component<{ children: React.ReactNode }, { error?: string }> {
  state: { error?: string } = {};
  static getDerivedStateFromError(e: unknown) { return { error: String(e) }; }
  render() { return this.state.error ? <div className="rounded-md border border-err/40 bg-err/10 p-3 text-sm text-err">Trace diff failed: {this.state.error}</div> : this.props.children; }
}

export default function TraceDiffPage() { return <Boundary><DiffInner /></Boundary>; }
