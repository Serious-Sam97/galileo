"use client";

import { useEffect, useState } from "react";

export type Theme = "system" | "dark" | "light";
const KEY = "galileo.theme";

export function getTheme(): Theme {
  try { const v = localStorage.getItem(KEY); if (v === "dark" || v === "light" || v === "system") return v; } catch {}
  return "system";
}
export function resolvedTheme(t: Theme): "dark" | "light" {
  if (t !== "system") return t;
  return typeof window !== "undefined" && window.matchMedia?.("(prefers-color-scheme: light)").matches ? "light" : "dark";
}
export function applyTheme(t: Theme) {
  const r = resolvedTheme(t);
  document.documentElement.dataset.theme = r;
  document.documentElement.style.colorScheme = r;
}
export function setTheme(t: Theme) {
  try { localStorage.setItem(KEY, t); } catch {}
  applyTheme(t);
  window.dispatchEvent(new Event("galileo-theme"));
}
export function useTheme(): [Theme, (t: Theme) => void, "dark" | "light"] {
  const [theme, set] = useState<Theme>("system");
  const [resolved, setResolved] = useState<"dark" | "light">("dark");
  useEffect(() => {
    const sync = () => { const t = getTheme(); set(t); setResolved(resolvedTheme(t)); applyTheme(t); };
    sync();
    window.addEventListener("galileo-theme", sync);
    const mq = window.matchMedia?.("(prefers-color-scheme: light)");
    mq?.addEventListener?.("change", sync);
    return () => { window.removeEventListener("galileo-theme", sync); mq?.removeEventListener?.("change", sync); };
  }, []);
  return [theme, setTheme, resolved];
}
/** Chart colors that follow the theme. */
export function chartTokens() {
  const cs = getComputedStyle(document.documentElement);
  const v = (n: string) => cs.getPropertyValue(n).trim();
  return { fg: v("--fg"), muted: v("--muted"), border: v("--border"), panel: v("--panel"), accent: v("--accent"), ok: v("--ok"), err: v("--err"), warn: v("--warn"), info: v("--info") };
}
