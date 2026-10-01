"use client";

import { useCallback, useSyncExternalStore } from "react";

const EVENT = "galileo-stored";
// Values set this session, so a choice still applies when storage is unavailable.
const memory = new Map<string, string>();

/**
 * A string remembered in this browser (localStorage), shared by every component that reads the same
 * key. The server render and the first client render see `fallback`. When storage throws (private
 * mode, blocked site data) the value lives in memory for the session instead.
 */
export function useStored(key: string, fallback: string): [string, (v: string) => void] {
  const subscribe = useCallback((cb: () => void) => {
    const on = (e: Event) => { if (!(e instanceof CustomEvent) || e.detail === key) cb(); };
    window.addEventListener(EVENT, on);
    window.addEventListener("storage", cb);
    return () => { window.removeEventListener(EVENT, on); window.removeEventListener("storage", cb); };
  }, [key]);
  const read = () => { try { return localStorage.getItem(key) ?? memory.get(key) ?? fallback; } catch { return memory.get(key) ?? fallback; } };
  const value = useSyncExternalStore(subscribe, read, () => fallback);
  const set = useCallback((v: string) => {
    memory.set(key, v);
    try { localStorage.setItem(key, v); } catch {}
    window.dispatchEvent(new CustomEvent(EVENT, { detail: key }));
  }, [key]);
  return [value, set];
}
