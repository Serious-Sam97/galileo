"use client";

import { useParams, useRouter } from "next/navigation";
import Link from "next/link";
import { useMemo, useState } from "react";
import { useProjectId, useProjectQuery, useProjectMutation } from "@/lib/hooks";
import { put, del } from "@/lib/api";
import { Button, Card, Table, Th, Td, Badge, Drawer, ErrorBox, Stat } from "@/components/ui";
import { Chart, axisStyle, type EChartsOption } from "@/components/charts/chart";
import { SloForm, draftOfSlo, type SloDraft } from "../form";
import type { Slo, SloResult } from "@/lib/types";
import { fmtNum } from "@/lib/format";
import { C, tooltipStyle } from "@/lib/palette";

export default function SloPage() {
  const { sloId } = useParams<{ sloId: string }>();
  const pid = useProjectId();
  const router = useRouter();
  const q = useProjectQuery<{ slo: Slo; result: SloResult }>(["slo", sloId], `/slos/${sloId}`);
  const [editing, setEditing] = useState(false);
  const update = useProjectMutation<SloDraft>((p, b) => put(`/api/projects/${p}/slos/${sloId}`, b), [["slo", sloId], ["slos"]]);
  const remove = useProjectMutation<void>((p) => del(`/api/projects/${p}/slos/${sloId}`), [["slos"]]);
  const r = q.data?.result;
  const option = useMemo<EChartsOption>(() => {
    if (!r) return {};
    return {
      tooltip: { trigger: "axis", ...tooltipStyle, valueFormatter: (v) => (v == null ? "–" : (v as number).toFixed(3) + "%") },
      grid: { left: 56, right: 16, top: 16, bottom: 28 },
      xAxis: { type: "category", data: r.series.map((p) => new Date(p.ts * 1000).toLocaleDateString(undefined, { month: "short", day: "numeric" })), ...axisStyle },
      yAxis: { type: "value", min: (v: { min: number }) => Math.floor(Math.min(v.min, q.data!.slo.target_pct) - 0.5), max: 100, ...axisStyle, axisLabel: { ...axisStyle.axisLabel, formatter: (v: number) => v + "%" } },
      series: [
        { name: "daily SLI", type: "line", data: r.series.map((p) => (p.sli == null ? null : p.sli * 100)), lineStyle: { color: C.cyan }, itemStyle: { color: C.cyan }, areaStyle: { color: "rgba(91,156,255,0.1)" } },
        { name: "target", type: "line", data: r.series.map(() => q.data!.slo.target_pct), lineStyle: { color: C.warn, type: "dashed", width: 1 }, showSymbol: false, itemStyle: { color: C.warn } },
      ],
    };
  }, [r, q.data]);
  if (q.error) return <ErrorBox error={q.error} />;
  if (!q.data || !r) return <div className="text-muted">Evaluating SLO…</div>;
  const s = q.data.slo;
  const budgetTone = r.budget_remaining_pct != null && r.budget_remaining_pct < 0 ? "err" : r.budget_remaining_pct != null && r.budget_remaining_pct < 25 ? "warn" : "ok";
  return (
    <div className="mx-auto max-w-[1400px] space-y-4">
      <div className="flex items-center gap-3">
        <Link href={`/p/${pid}/slos`} className="text-muted hover:text-fg text-xs">← SLOs</Link>
        <h1 className="text-xl font-semibold tracking-tight">{s.name}</h1>
        <Badge tone={r.state === "ok" ? "ok" : "err"}>{r.state}</Badge>
        <div className="ml-auto flex gap-2">
          <Button size="sm" onClick={() => setEditing(true)}>Edit</Button>
          <Button size="sm" variant="danger" onClick={async () => { if (confirm("Delete SLO?")) { await remove.mutateAsync(); router.replace(`/p/${pid}/slos`); } }}>Delete</Button>
        </div>
      </div>
      {s.description && <p className="text-muted text-sm">{s.description}</p>}
      <div className="grid grid-cols-2 gap-3 md:grid-cols-5">
        <Stat label="SLI" value={r.sli_pct != null ? r.sli_pct.toFixed(3) + "%" : "–"} sub={`target ${s.target_pct}% over ${s.window_days}d`} tone={r.sli_pct != null && r.sli_pct < s.target_pct ? "err" : "ok"} />
        <Stat label="Error budget left" value={r.budget_remaining_pct != null ? r.budget_remaining_pct.toFixed(1) + "%" : "–"} tone={budgetTone} />
        <Stat label="Total events" value={fmtNum(r.total)} />
        <Stat label="Bad events" value={fmtNum(r.bad)} sub={`${fmtNum(r.allowed_bad)} allowed`} tone={r.bad > r.allowed_bad ? "err" : undefined} />
        <Stat label="Burn alerts" value={r.burn_alerts.filter((a) => a.triggered).length + " / " + r.burn_alerts.length} sub="firing" />
      </div>
      <Card title="Daily SLI"><Chart option={option} height={220} /></Card>
      <Card title="Burn-rate alerts">
        <Table><thead><tr><Th>Alert</Th><Th className="text-right">Long window burn</Th><Th className="text-right">Short window burn</Th><Th className="text-right">Threshold</Th><Th>State</Th></tr></thead>
          <tbody>{r.burn_alerts.map((a) => <tr key={a.name}><Td>{a.name}</Td><Td className="text-right font-mono">{a.long_burn != null ? a.long_burn.toFixed(2) + "×" : "–"}</Td><Td className="text-right font-mono">{a.short_burn != null ? a.short_burn.toFixed(2) + "×" : "–"}</Td><Td className="text-right font-mono">{a.burn_rate}×</Td><Td><Badge tone={a.triggered ? "err" : "ok"}>{a.triggered ? "firing" : "ok"}</Badge></Td></tr>)}</tbody></Table>
      </Card>
      <Drawer open={editing} onClose={() => setEditing(false)} title="Edit SLO" width="w-[720px]">
        <SloForm initial={draftOfSlo(s)} error={update.error} onSubmit={async (d) => { await update.mutateAsync(d); setEditing(false); }} />
      </Drawer>
    </div>
  );
}
