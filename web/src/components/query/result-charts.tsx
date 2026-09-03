"use client";

import { useMemo } from "react";
import { Chart, axisStyle, type EChartsOption } from "@/components/charts/chart";
import type { QueryResponse, Group, Deploy } from "@/lib/types";
import { colorFor, fmtNum } from "@/lib/format";
import { useProjectQuery } from "@/lib/hooks";

/** Deploys inside the chart's range, for vertical markers. */
export function useDeploys(start: string, end: string): Deploy[] {
  const span = Math.max(60, Math.ceil((Date.now() - new Date(start).getTime()) / 1000));
  const q = useProjectQuery<{ deploys: Deploy[] }>(["deploys", span], `/deploys?last_seconds=${span}`, { refetchInterval: 60_000 });
  const s = new Date(start).getTime(); const e = new Date(end).getTime();
  return (q.data?.deploys ?? []).filter((d) => { const t = new Date(d.at).getTime(); return t >= s && t <= e; });
}

export function groupLabel(g: Group, breakdowns: string[]): string {
  if (!g.key.length) return "all";
  return g.key.map((k, i) => `${breakdowns[i]?.split(".").pop() ?? ""}=${k || "∅"}`).join(", ");
}

/** One line chart per calculation, one series per group. */
export function SeriesCharts({ res, onBrush, annotations, onPointClick, height }: { res: QueryResponse; onBrush?: (start: number, end: number) => void; annotations?: { ts: number; text: string }[]; onPointClick?: (ts: number) => void; height?: number }) {
  return (
    <div className="grid gap-3" style={{ gridTemplateColumns: res.calculations.length > 1 ? "repeat(auto-fit, minmax(420px, 1fr))" : "1fr" }}>
      {res.calculations.map((label, ci) => (
        <SeriesChart key={label} res={res} ci={ci} label={label} onBrush={onBrush} annotations={annotations} onPointClick={onPointClick} height={height} />
      ))}
    </div>
  );
}

function SeriesChart({ res, ci, label, onBrush, annotations, onPointClick, height }: { res: QueryResponse; ci: number; label: string; onBrush?: (s: number, e: number) => void; annotations?: { ts: number; text: string }[]; onPointClick?: (ts: number) => void; height?: number }) {
  const deploys = useDeploys(res.start, res.end);
  const option = useMemo<EChartsOption>(() => {
    const marks = [
      ...deploys.map((d) => ({ xAxis: new Date(d.at).getTime(), name: `deploy ${d.version}`, lineStyle: { color: "#c084fc" }, label: { color: "#c084fc" } })),
      ...(annotations ?? []).map((a) => ({ xAxis: a.ts * 1000, name: `✎ ${a.text.slice(0, 40)}`, lineStyle: { color: "#f5a524" }, label: { color: "#f5a524" } })),
    ];
    const markLine = marks.length ? {
      silent: false, symbol: ["none", "none"], lineStyle: { type: "dashed" as const, width: 1 },
      label: { show: true, position: "insideEndTop" as const, formatter: (p: { name: string }) => p.name, fontSize: 9 },
      data: marks,
    } : undefined;
    const shown = res.groups.slice(0, 20);
    const series = shown.map((g, gi) => ({
      name: groupLabel(g, res.breakdowns),
      type: "line" as const,
      showSymbol: false,
      lineStyle: { width: 1.5, color: colorFor(gi) },
      itemStyle: { color: colorFor(gi) },
      data: g.series.map((p) => [p.ts * 1000, p.values[ci]]),
      connectNulls: false,
      ...(gi === 0 && markLine ? { markLine } : {}),
      ...(onPointClick ? { triggerLineEvent: true, symbolSize: 6 } : {}),
    }));
    // dashed ghost lines from the comparison window (aligned onto the current axis)
    const ghosts = shown.filter((g) => g.compare_series && g.compare_series.length).map((g, gi) => ({
      name: `${groupLabel(g, res.breakdowns)} (prev)`,
      type: "line" as const,
      showSymbol: false,
      lineStyle: { width: 1, color: colorFor(gi), type: "dashed" as const, opacity: 0.7 },
      itemStyle: { color: colorFor(gi) },
      data: (g.compare_series ?? []).map((p) => [p.ts * 1000, p.values[ci]]),
      connectNulls: false,
      tooltip: { show: true },
    }));
    return {
      title: { text: label, left: 8, top: 4, textStyle: { fontSize: 12, color: "#e6e9ef", fontWeight: 500 } },
      tooltip: { trigger: "axis", backgroundColor: "#161c29", borderColor: "#232a3a", textStyle: { color: "#e6e9ef", fontSize: 11 }, valueFormatter: (v) => fmtNum(v as number) },
      xAxis: { type: "time", ...axisStyle },
      yAxis: { type: "value", ...axisStyle, axisLabel: { ...axisStyle.axisLabel, formatter: (v: number) => fmtNum(v, 1) } },
      brush: onBrush ? { toolbox: ["lineX", "clear"], xAxisIndex: 0, brushStyle: { color: "rgba(245,165,36,0.15)", borderColor: "#f5a524" } } : undefined,
      toolbox: onBrush ? { show: true, right: 8, top: 0, feature: { brush: { type: ["lineX", "clear"], title: { lineX: "Select time range", clear: "Clear" } } }, iconStyle: { borderColor: "#8b93a7" } } : undefined,
      series: [...series, ...ghosts],
    };
  }, [res, ci, label, onBrush, deploys, annotations]);
  const events = useMemo(() => {
    const ev: Record<string, (p: unknown) => void> = {};
    if (onBrush) ev.brushEnd = (p: unknown) => {
      const areas = (p as { areas?: { coordRange?: number[] }[] }).areas ?? [];
      const r = areas[0]?.coordRange;
      if (r && r.length === 2) onBrush(r[0] / 1000, r[1] / 1000);
    };
    if (onPointClick) ev.click = (p: unknown) => { const v = (p as { value?: [number, number] }).value; if (v && typeof v[0] === "number") onPointClick(v[0] / 1000); };
    return Object.keys(ev).length ? ev : undefined;
  }, [onBrush, onPointClick]);
  return <Chart option={option} height={height ?? 260} onEvents={events} />;
}

export function HeatmapChart({ res, onBrush }: { res: QueryResponse; onBrush?: (sel: { tStart: number; tEnd: number; lo: number; hi: number }) => void }) {
  const hm = res.heatmap!;
  const option = useMemo<EChartsOption>(() => {
    const data: [number, number, number][] = [];
    hm.counts.forEach((row, bi) => row.forEach((c, vi) => c > 0 && data.push([bi, vi, c])));
    const yLabels = hm.bin_edges.map((e) => fmtNum(e, 1));
    return {
      title: { text: `HEATMAP(${hm.field}) — ${hm.total} events${hm.log_scale ? ", log scale" : ""}`, left: 8, top: 4, textStyle: { fontSize: 12, color: "#e6e9ef", fontWeight: 500 } },
      tooltip: {
        backgroundColor: "#161c29", borderColor: "#232a3a", textStyle: { color: "#e6e9ef", fontSize: 11 },
        formatter: (p: unknown) => {
          const v = (p as { value: [number, number, number] }).value;
          const lo = hm.bin_edges[v[1]]; const hi = hm.bin_edges[v[1] + 1];
          return `${new Date(hm.buckets[v[0]] * 1000).toLocaleTimeString()}<br/>${fmtNum(lo, 1)} – ${hi !== undefined ? fmtNum(hi, 1) : "max"}<br/><b>${v[2]}</b> events`;
        },
      },
      grid: { left: 56, right: 16, top: 28, bottom: 28 },
      xAxis: { type: "category", data: hm.buckets.map((b) => new Date(b * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })), ...axisStyle, splitLine: { show: false } },
      yAxis: { type: "category", data: yLabels, ...axisStyle, splitLine: { show: false } },
      visualMap: { min: 0, max: Math.max(1, hm.max_count), show: false, inRange: { color: ["#1a2030", "#3a3f6b", "#5b9cff", "#f5a524", "#ff5c6c"] } },
      brush: onBrush ? { toolbox: ["rect", "clear"], xAxisIndex: 0, yAxisIndex: 0, brushStyle: { color: "rgba(245,165,36,0.2)", borderColor: "#f5a524" } } : undefined,
      toolbox: onBrush ? { show: true, right: 8, top: 0, feature: { brush: { type: ["rect", "clear"], title: { rect: "BubbleUp: select a region", clear: "Clear" } } }, iconStyle: { borderColor: "#8b93a7" } } : undefined,
      series: [{ type: "heatmap", data, emphasis: { itemStyle: { borderColor: "#fff", borderWidth: 1 } } }],
    };
  }, [hm, onBrush]);
  const events = useMemo(() => {
    if (!onBrush) return undefined;
    return {
      brushEnd: (p: unknown) => {
        const areas = (p as { areas?: { coordRange?: number[][] }[] }).areas ?? [];
        const r = areas[0]?.coordRange;
        if (!r || r.length !== 2) return;
        const [xr, yr] = r;
        const bi0 = Math.max(0, Math.round(xr[0])); const bi1 = Math.min(hm.buckets.length - 1, Math.round(xr[1]));
        const vi0 = Math.max(0, Math.round(yr[0])); const vi1 = Math.min(hm.bin_edges.length - 1, Math.round(yr[1]));
        const tStart = hm.buckets[bi0]; const tEnd = hm.buckets[bi1] + res.granularity;
        const lo = hm.bin_edges[vi0]; const hi = vi1 + 1 < hm.bin_edges.length ? hm.bin_edges[vi1 + 1] : Number.POSITIVE_INFINITY;
        onBrush({ tStart, tEnd, lo, hi });
      },
    };
  }, [onBrush, hm, res.granularity]);
  return <Chart option={option} height={320} onEvents={events} />;
}
