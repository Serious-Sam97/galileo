"use client";

import { useState } from "react";
import clsx from "clsx";
import { useProjectQuery, useRunQuery } from "@/lib/hooks";
import { Input, Select, Empty, ErrorBox, Badge, PageHeader } from "@/components/ui";
import { SeriesCharts } from "@/components/query/result-charts";
import type { CalcOp, Query, TimeRange } from "@/lib/types";
import { useLastSeconds } from "@/lib/time-range";

interface MetricMeta { name: string; kind: string; unit: string; description: string; services: number; points: number }

export default function MetricsPage() {
  const [last] = useLastSeconds();
  const range: TimeRange = { last_seconds: last };
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<MetricMeta | null>(null);
  const [agg, setAgg] = useState<CalcOp>("AVG");
  const [groupBy, setGroupBy] = useState("host.name");
  const names = useProjectQuery<{ metrics: MetricMeta[] }>(["metric-names"], `/metrics/names?last_seconds=${7 * 86400}`);
  const field = selected?.kind === "histogram" || selected?.kind === "exponential_histogram" ? "mean" : "value";
  const q: Query | null = selected ? { dataset: "metrics", time_range: range, calculations: [{ op: agg, field }], filters: [{ field: "name", op: "eq", value: selected.name }], breakdowns: groupBy ? [groupBy] : [], orders: [], limit: 20 } : null;
  const res = useRunQuery(q);
  const list = (names.data?.metrics ?? []).filter((m) => m.name.includes(filter));

  return (
    <div className="mx-auto max-w-[1400px] space-y-4">
    <PageHeader title="Metrics" sub="Every metric your services send: runtime, host, requests, pools and your own." />
    <div className="grid gap-3" style={{ gridTemplateColumns: "320px 1fr" }}>
      <div className="rounded-xl border bg-panel/80 overflow-hidden">
        <div className="border-b p-2"><Input placeholder="filter metrics" value={filter} onChange={(e) => setFilter(e.target.value)} /></div>
        <div className="max-h-[calc(100vh-160px)] overflow-auto scroll-thin">
          {list.length === 0 && <Empty>No metrics received in the last 7 days.</Empty>}
          {list.map((m) => (
            <button key={m.name} onClick={() => setSelected(m)} className={clsx("block w-full border-b border-border/50 px-3 py-2 text-left hover:bg-panel-2", selected?.name === m.name && "nav-active")}>
              <div className="font-mono text-[12px] truncate">{m.name}</div>
              <div className="flex gap-2 text-[10px] text-muted mt-0.5"><Badge>{m.kind}</Badge>{m.unit && <span>{m.unit}</span>}<span>{m.points} points</span></div>
            </button>
          ))}
        </div>
      </div>
      <div className="space-y-3">
        <div className="flex flex-wrap items-center gap-2 rounded-xl border bg-panel/80 p-3">
          <Select value={agg} onChange={(e) => setAgg(e.target.value as CalcOp)}>{["AVG", "MAX", "MIN", "SUM", "P95", "P99", "COUNT"].map((o) => <option key={o}>{o}</option>)}</Select>
          <span className="text-xs text-muted">of {field}</span>
          <Input className="w-44" placeholder="group by attribute" value={groupBy} onChange={(e) => setGroupBy(e.target.value)} />
        </div>
        {!selected && <Empty>Pick a metric on the left.</Empty>}
        <ErrorBox error={res.error} />
        {selected && res.data && (res.data.groups.length ? <SeriesCharts res={res.data} /> : <Empty>No points in this range.</Empty>)}
        {selected?.description && <div className="text-xs text-muted">{selected.description}</div>}
      </div>
    </div>
    </div>
  );
}
