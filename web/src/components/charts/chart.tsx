"use client";

import { C } from "@/lib/palette";

import * as echarts from "echarts";
import { useEffect, useRef } from "react";

export type EChartsOption = echarts.EChartsOption;

export function Chart({ option, height = 260, onEvents, className }: { option: EChartsOption; height?: number | string; onEvents?: Record<string, (p: unknown) => void>; className?: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const inst = useRef<echarts.ECharts | null>(null);
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
  }, [option, onEvents]);
  return <div ref={ref} className={className} style={{ height, width: "100%" }} />;
}

const reducedMotion = () => typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;

function baseOption(): EChartsOption {
  return {
    backgroundColor: "transparent",
    textStyle: { color: C.muted, fontFamily: "inherit", fontSize: 11 },
    // a short draw-in on first render; updates (refetches) stay snappy
    animation: !reducedMotion(),
    animationDuration: 450,
    animationDurationUpdate: 200,
    grid: { left: 48, right: 16, top: 24, bottom: 28, containLabel: false },
  };
}

export const axisStyle = {
  axisLine: { lineStyle: { color: C.border } },
  axisTick: { show: false },
  axisLabel: { color: C.faint, fontSize: 10 },
  splitLine: { lineStyle: { color: C.grid } },
};
