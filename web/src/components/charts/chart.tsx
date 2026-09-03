"use client";

import { chartTokens } from "@/lib/theme";

import * as echarts from "echarts";
import { useState, useEffect, useRef } from "react";

export type EChartsOption = echarts.EChartsOption;

export function Chart({ option, height = 260, onEvents, className }: { option: EChartsOption; height?: number | string; onEvents?: Record<string, (p: unknown) => void>; className?: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const inst = useRef<echarts.ECharts | null>(null);
  const [themeTick, setThemeTick] = useState(0);
  useEffect(() => { const f = () => setThemeTick((x) => x + 1); window.addEventListener("galileo-theme", f); return () => window.removeEventListener("galileo-theme", f); }, []);
  useEffect(() => {
    if (!ref.current) return;
    const chart = echarts.init(ref.current, undefined, { renderer: "canvas" });
    inst.current = chart;
    const ro = new ResizeObserver(() => chart.resize());
    ro.observe(ref.current);
    return () => {
      ro.disconnect();
      chart.dispose();
      inst.current = null;
    };
  }, []);
  useEffect(() => {
    const chart = inst.current;
    if (!chart) return;
    chart.setOption({ ...baseOption(), ...option }, { notMerge: true });
    chart.off("click");
    chart.off("brushEnd");
    chart.off("brushselected");
    if (onEvents) for (const [k, fn] of Object.entries(onEvents)) chart.on(k, fn);
  }, [option, onEvents, themeTick]);
  return <div ref={ref} className={className} style={{ height, width: "100%" }} />;
}

function baseOption(): EChartsOption {
  const tok = chartTokens();
  axisStyle.axisLine.lineStyle.color = tok.border;
  axisStyle.axisLabel.color = tok.muted;
  axisStyle.splitLine.lineStyle.color = tok.border;
  return {
    backgroundColor: "transparent",
    textStyle: { color: tok.muted, fontFamily: "inherit", fontSize: 11 },
    animation: false,
    grid: { left: 48, right: 16, top: 24, bottom: 28, containLabel: false },
  };
}

export const axisStyle = {
  axisLine: { lineStyle: { color: "#232a3a" } },
  axisTick: { show: false },
  axisLabel: { color: "#8b93a7", fontSize: 10 },
  splitLine: { lineStyle: { color: "#1a2030" } },
};
