"use client";

import { useMemo, useState } from "react";
import { useRouter } from "next/navigation";
import { useProjectId, useProjectQuery } from "@/lib/hooks";
import { Card, Empty, ErrorBox, Select, Table, Th, Td, Badge } from "@/components/ui";
import { Chart, type EChartsOption } from "@/components/charts/chart";
import { fmtNum, fmtMs, encodeQ } from "@/lib/format";
import { defaultQuery, type ServiceMapRes } from "@/lib/types";

const KIND_COLOR: Record<string, string> = { service: "#5b9cff", db: "#34d399", llm: "#c084fc", http: "#f5a524" };

export default function ServiceMapPage() {
  const pid = useProjectId();
  const router = useRouter();
  const [last, setLast] = useState(3600);
  const q = useProjectQuery<ServiceMapRes>(["service-map", last], `/service-map?last_seconds=${last}`);
  const d = q.data;
  const graph = useMemo(() => {
    if (!d) return null;
    const nodes = new Map<string, { id: string; kind: string; spans: number; errors: number; p95_ms: number }>();
    for (const n of d.nodes) nodes.set(n.id, { id: n.id, kind: "service", spans: Number(n.spans), errors: Number(n.errors), p95_ms: Number(n.p95_ms) });
    const edges: { src: string; dst: string; calls: number; errors: number; p95_ms: number; kind: string }[] = [];
    for (const e of d.edges) edges.push({ src: e.src, dst: e.dst, calls: Number(e.calls), errors: Number(e.errors), p95_ms: Number(e.p95_ms), kind: "service" });
    for (const l of d.leaves) {
      const kind = l.kind ?? "http";
      if (kind === "http" && nodes.has(l.dst)) continue; // already a service
      const id = kind === "http" ? "http" : l.dst;
      const cur = nodes.get(id) ?? { id, kind, spans: 0, errors: 0, p95_ms: 0 };
      cur.spans += Number(l.calls); cur.errors += Number(l.errors); cur.p95_ms = Math.max(cur.p95_ms, Number(l.p95_ms));
      nodes.set(id, cur);
      edges.push({ src: l.src, dst: id, calls: Number(l.calls), errors: Number(l.errors), p95_ms: Number(l.p95_ms), kind });
    }
    return { nodes: [...nodes.values()], edges };
  }, [d]);

  const option = useMemo<EChartsOption>(() => {
    if (!graph) return {};
    const maxSpans = Math.max(1, ...graph.nodes.map((n) => n.spans));
    return {
      tooltip: { backgroundColor: "#161c29", borderColor: "#232a3a", textStyle: { color: "#e6e9ef", fontSize: 11 },
        formatter: (p: unknown) => { const x = p as { dataType: string; data: Record<string, unknown> }; if (x.dataType === "edge") { const e = x.data as { source: string; target: string; calls: number; errors: number; p95_ms: number }; return `${e.source} → ${e.target}<br/>${fmtNum(e.calls, 0)} calls · p95 ${fmtMs(e.p95_ms)} · ${e.calls ? Math.round(100 * e.errors / e.calls) : 0}% errors`; } const n = x.data as { name: string; spans: number; errors: number; p95_ms: number; kind: string }; return `<b>${n.name}</b> (${n.kind})<br/>${fmtNum(n.spans, 0)} spans · p95 ${fmtMs(n.p95_ms)} · ${n.spans ? Math.round(100 * n.errors / n.spans) : 0}% errors`; } },
      series: [{
        type: "graph", layout: "force", roam: true, draggable: true, force: { repulsion: 900, edgeLength: [120, 220], gravity: 0.08 },
        label: { show: true, position: "bottom", color: "#e6e9ef", fontSize: 11 },
        edgeSymbol: ["none", "arrow"], edgeSymbolSize: 8,
        edgeLabel: { show: true, fontSize: 9, color: "#8b93a7", formatter: (p: unknown) => { const e = (p as { data: { calls: number; p95_ms: number } }).data; return `${fmtNum(e.calls, 0)} · ${fmtMs(e.p95_ms)}`; } },
        lineStyle: { color: "#3a4256", curveness: 0.15, width: 1.5 },
        data: graph.nodes.map((n) => ({ name: n.id, kind: n.kind, spans: n.spans, errors: n.errors, p95_ms: n.p95_ms, symbolSize: 18 + 30 * Math.sqrt(n.spans / maxSpans), symbol: n.kind === "db" ? "roundRect" : n.kind === "llm" ? "diamond" : n.kind === "http" ? "triangle" : "circle",
          itemStyle: { color: KIND_COLOR[n.kind] ?? "#5b9cff", borderColor: n.spans && n.errors / n.spans > 0.05 ? "#ff5c6c" : "#0d1017", borderWidth: 2 } })),
        links: graph.edges.map((e) => ({ source: e.src, target: e.dst, calls: e.calls, errors: e.errors, p95_ms: e.p95_ms, lineStyle: { color: e.calls && e.errors / e.calls > 0.05 ? "#ff5c6c" : "#3a4256", width: 1 + Math.min(4, Math.log10(1 + e.calls)) } })),
      }],
    };
  }, [graph]);

  const events = useMemo(() => ({
    click: (p: unknown) => {
      const x = p as { dataType: string; data: { name: string; kind: string } };
      if (x.dataType !== "node" || x.data.kind !== "service") return;
      const q = { ...defaultQuery("spans"), time_range: { last_seconds: last }, filters: [{ field: "service_name", op: "eq" as const, value: x.data.name }], breakdowns: ["http_route"], calculations: [{ op: "COUNT" as const }, { op: "P95" as const, field: "duration_ms" }] };
      router.push(`/p/${pid}/query?q=${encodeQ(q)}`);
    },
  }), [last, pid, router]);

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2"><h1 className="text-base font-semibold mr-2">Service map</h1>
        <Select value={last} onChange={(e) => setLast(Number(e.target.value))}>{[900, 3600, 6 * 3600, 86400, 7 * 86400].map((x) => <option key={x} value={x}>Last {x >= 86400 ? x / 86400 + "d" : x >= 3600 ? x / 3600 + "h" : x / 60 + "m"}</option>)}</Select>
        <span className="text-xs text-muted">Edges come from parent → child spans across services and from DB / LLM / HTTP client spans. Click a service to query it. Red border = error rate over 5%.</span></div>
      <ErrorBox error={q.error} />
      {graph && graph.nodes.length === 0 && <Empty>No spans in this window.</Empty>}
      {graph && graph.nodes.length > 0 && <Card><Chart option={option} height={520} onEvents={events} /></Card>}
      {graph && graph.edges.length > 0 && (
        <Card title="Edges">
          <Table><thead><tr><Th>From</Th><Th>To</Th><Th>Kind</Th><Th className="text-right">Calls</Th><Th className="text-right">p95</Th><Th className="text-right">Errors</Th></tr></thead>
            <tbody>{[...graph.edges].sort((a, b) => b.calls - a.calls).map((e, i) => <tr key={i}><Td className="font-mono">{e.src}</Td><Td className="font-mono">{e.dst}</Td><Td><Badge tone={e.kind === "db" ? "ok" : e.kind === "llm" ? "accent" : "muted"}>{e.kind}</Badge></Td><Td className="text-right">{fmtNum(e.calls, 0)}</Td><Td className="text-right font-mono">{fmtMs(e.p95_ms)}</Td><Td className="text-right">{e.errors ? <span className="text-err">{fmtNum(e.errors, 0)} ({Math.round(100 * e.errors / e.calls)}%)</span> : "0"}</Td></tr>)}</tbody></Table>
        </Card>
      )}
    </div>
  );
}
