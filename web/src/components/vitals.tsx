import clsx from "clsx";

/** Web Vitals thresholds (web.dev): [good, poor]. */
export const VITAL_THRESHOLDS: Record<string, [number, number]> = { lcp: [2500, 4000], cls: [0.1, 0.25], inp: [200, 500], fid: [100, 300], fcp: [1800, 3000], ttfb: [800, 1800] };
export const VITAL_LABELS: Record<string, string> = { lcp: "LCP", cls: "CLS", inp: "INP", fid: "FID", fcp: "FCP", ttfb: "TTFB" };

export function vitalTone(name: string, v: number | undefined): "ok" | "warn" | "err" | undefined {
  const t = VITAL_THRESHOLDS[name]; if (!t || v == null) return undefined;
  return v <= t[0] ? "ok" : v <= t[1] ? "warn" : "err";
}
export function fmtVital(name: string, v: number | undefined): string {
  if (v == null || Number.isNaN(v)) return "–";
  return name === "cls" ? v.toFixed(3) : v >= 1000 ? `${(v / 1000).toFixed(2)}s` : `${Math.round(v)}ms`;
}
export function VitalPill({ name, value }: { name: string; value: number | undefined }) {
  const tone = vitalTone(name, value);
  return <span className={clsx("inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] font-mono", tone === "ok" && "bg-ok/15 text-ok", tone === "warn" && "bg-warn/15 text-warn", tone === "err" && "bg-err/15 text-err", !tone && "bg-panel-2 text-muted")}>{VITAL_LABELS[name] ?? name} {fmtVital(name, value)}</span>;
}
