"use client";

import Link from "next/link";
import { useState } from "react";
import { useProjectId, useProjectQuery, useRunQuery } from "@/lib/hooks";
import { Card, Stat, Table, Th, Td, Empty, Badge } from "@/components/ui";
import { SeriesCharts } from "@/components/query/result-charts";
import { fmtMs, fmtNum, fmtUsd, ago, encodeQ } from "@/lib/format";
import type { NPlusOne, Query } from "@/lib/types";
import { TimeRangePicker } from "@/components/time-range";

interface ServiceRow { service_name: string; spans: number; requests: number; errors: number; p50_ms: number; p95_ms: number; p99_ms: number; last_seen: string; llm_calls: number; llm_cost_usd: number }

export default function OverviewPage() {
  const pid = useProjectId();
  const [last, setLast] = useState(3600);
  const services = useProjectQuery<{ services: ServiceRow[] }>(["services", last], `/services?last_seconds=${last}`, { refetchInterval: 30_000 });
  const q: Query = { dataset: "spans", time_range: { last_seconds: last }, calculations: [{ op: "COUNT" }, { op: "P99", field: "duration_ms" }, { op: "AVG", field: "is_error" }], filters: [{ field: "is_root", op: "eq", value: 1 }], breakdowns: ["service.name"], orders: [], limit: 8 };
  const series = useRunQuery(q, { refetchInterval: 30_000 });
  const nplus = useProjectQuery<{ candidates: NPlusOne[] }>(["nplusone", last], `/db/nplusone?last_seconds=${last}`, { refetchInterval: 60_000 });
  const rows = services.data?.services ?? [];
  const totals = rows.reduce((a, r) => ({ req: a.req + r.requests, err: a.err + r.errors, llm: a.llm + r.llm_calls, cost: a.cost + r.llm_cost_usd }), { req: 0, err: 0, llm: 0, cost: 0 });

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-base font-semibold">Overview</h1>
        <TimeRangePicker value={{ last_seconds: last }} onChange={(t) => "last_seconds" in t && setLast(t.last_seconds)} />
      </div>
      <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
        <Stat label="Requests" value={fmtNum(totals.req)} sub="root spans" />
        <Stat label="Error rate" value={totals.req ? ((totals.err / totals.req) * 100).toFixed(2) + "%" : "–"} tone={totals.req && totals.err / totals.req > 0.05 ? "err" : undefined} sub={`${totals.err} errors`} />
        <Stat label="LLM calls" value={fmtNum(totals.llm)} sub={fmtUsd(totals.cost) + " spent"} />
        <Stat label="Services" value={rows.length} sub={rows.map((r) => r.service_name).slice(0, 4).join(", ")} />
      </div>
      {series.data && series.data.groups.length > 0 ? <SeriesCharts res={series.data} /> : <Empty>No traces yet. <Link href={`/p/${pid}/welcome`} className="underline text-accent">Get started</Link> — pick your stack, get a key and see the first trace in minutes.</Empty>}
      {!!nplus.data?.candidates.length && (
        <Card title="Repeated queries (N+1 candidates)">
          <Table className="max-h-72">
            <thead><tr><Th>Statement</Th><Th>Called from</Th><Th>Route</Th><Th className="text-right">Traces</Th><Th className="text-right">Avg repeats</Th><Th className="text-right">Avg DB time</Th><Th></Th></tr></thead>
            <tbody>{nplus.data.candidates.map((c, i) => (
              <tr key={i} className="hover:bg-panel-2">
                <Td className="font-mono text-[11px] max-w-[420px] truncate" title={c.statement}>{c.statement}</Td>
                <Td className="font-mono text-[11px]">{c.namespace ? `${c.namespace}.` : ""}{c.function || <span className="text-muted">(unknown)</span>}<div className="text-[10px] text-muted">{c.file}</div></Td>
                <Td className="font-mono text-[11px] text-muted">{c.sample_route}{c.routes > 1 ? ` +${c.routes - 1}` : ""}</Td>
                <Td className="text-right tabular-nums">{fmtNum(c.traces)}</Td>
                <Td className="text-right tabular-nums">{c.avg_repeats}× (max {c.max_repeats})</Td>
                <Td className="text-right font-mono">{fmtMs(c.avg_ms_per_trace)}</Td>
                <Td><Link href={`/p/${pid}/traces/${c.sample_trace}`} className="text-info hover:underline text-[11px]">sample trace</Link></Td>
              </tr>
            ))}</tbody>
          </Table>
        </Card>
      )}
      <Card title="Services">
        {rows.length === 0 ? <Empty>No services seen in this window.</Empty> : (
          <Table>
            <thead><tr><Th>Service</Th><Th className="text-right">Requests</Th><Th className="text-right">Errors</Th><Th className="text-right">p50</Th><Th className="text-right">p95</Th><Th className="text-right">p99</Th><Th className="text-right">LLM calls</Th><Th className="text-right">LLM cost</Th><Th>Last seen</Th></tr></thead>
            <tbody>
              {rows.map((r) => {
                const errPct = r.requests ? r.errors / r.requests : 0;
                const link = encodeQ({ ...q, breakdowns: ["http.route"], filters: [{ field: "service.name", op: "eq", value: r.service_name }, { field: "is_root", op: "eq", value: 1 }], limit: 20 });
                return (
                  <tr key={r.service_name} className="hover:bg-panel-2">
                    <Td><Link href={`/p/${pid}/query?q=${link}`} className="font-medium hover:text-accent">{r.service_name}</Link></Td>
                    <Td className="text-right tabular-nums">{fmtNum(r.requests)}</Td>
                    <Td className="text-right tabular-nums">{r.errors > 0 ? <Badge tone={errPct > 0.05 ? "err" : "warn"}>{r.errors} ({(errPct * 100).toFixed(1)}%)</Badge> : <span className="text-muted">0</span>}</Td>
                    <Td className="text-right tabular-nums font-mono">{fmtMs(r.p50_ms)}</Td>
                    <Td className="text-right tabular-nums font-mono">{fmtMs(r.p95_ms)}</Td>
                    <Td className="text-right tabular-nums font-mono">{fmtMs(r.p99_ms)}</Td>
                    <Td className="text-right tabular-nums">{fmtNum(r.llm_calls)}</Td>
                    <Td className="text-right tabular-nums">{fmtUsd(r.llm_cost_usd)}</Td>
                    <Td className="text-muted">{ago(r.last_seen)}</Td>
                  </tr>
                );
              })}
            </tbody>
          </Table>
        )}
      </Card>
    </div>
  );
}
