export function fmtNum(v: number | null | undefined, digits = 2): string {
  if (v === null || v === undefined || Number.isNaN(v)) return "–";
  const a = Math.abs(v);
  if (a >= 1e9) return (v / 1e9).toFixed(digits) + "B";
  if (a >= 1e6) return (v / 1e6).toFixed(digits) + "M";
  if (a >= 1e4) return (v / 1e3).toFixed(1) + "k";
  if (Number.isInteger(v)) return v.toString();
  if (a < 0.01 && a > 0) return v.toExponential(1);
  return v.toFixed(digits);
}

export function fmtMs(ms: number | null | undefined): string {
  if (ms === null || ms === undefined) return "–";
  if (ms < 1) return (ms * 1000).toFixed(0) + "µs";
  if (ms < 1000) return ms.toFixed(ms < 10 ? 2 : 0) + "ms";
  if (ms < 60_000) return (ms / 1000).toFixed(2) + "s";
  return (ms / 60_000).toFixed(1) + "m";
}

export function fmtUsd(v: number | null | undefined): string {
  if (v === null || v === undefined) return "–";
  if (v === 0) return "$0";
  if (v < 0.01) return "$" + v.toFixed(5);
  return "$" + v.toFixed(v < 1 ? 4 : 2);
}

export function fmtDuration(secs: number): string {
  if (secs % 86400 === 0) return secs / 86400 + "d";
  if (secs % 3600 === 0) return secs / 3600 + "h";
  if (secs % 60 === 0) return secs / 60 + "m";
  return secs + "s";
}

export function fmtTime(iso: string | number | Date): string {
  const d = typeof iso === "number" ? new Date(iso * 1000) : new Date(iso);
  return d.toLocaleString(undefined, { month: "short", day: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

export function ago(iso: string | null | undefined): string {
  if (!iso) return "never";
  const s = (Date.now() - new Date(iso).getTime()) / 1000;
  if (s < 60) return `${Math.floor(s)}s ago`;
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

export { SERIES as PALETTE, colorFor } from "./palette";

/** Encode a query into a URL-safe string and back. */
export function encodeQ(q: unknown): string {
  return btoa(unescape(encodeURIComponent(JSON.stringify(q)))).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}
export function decodeQ<T>(s: string | null): T | null {
  if (!s) return null;
  try {
    const b = s.replace(/-/g, "+").replace(/_/g, "/");
    return JSON.parse(decodeURIComponent(escape(atob(b)))) as T;
  } catch {
    return null;
  }
}
