"use client";

import { createContext, useCallback, useContext } from "react";
import { useStored } from "./stored";

/** The windows every page offers, in seconds. */
export const RANGE_PRESETS = [
  [900, "Last 15 min"], [3600, "Last hour"], [4 * 3600, "Last 4 hours"], [24 * 3600, "Last 24 hours"],
  [3 * 86400, "Last 3 days"], [7 * 86400, "Last 7 days"], [30 * 86400, "Last 30 days"],
] as const;

const KEY = "galileo.range";
const DEFAULT = 24 * 3600;

const Ctx = createContext<[number, (s: number) => void]>([DEFAULT, () => {}]);

/** One time window for the whole project, picked in the top bar and remembered per browser. */
export function TimeRangeProvider({ children }: { children: React.ReactNode }) {
  const [stored, store] = useStored(KEY, String(DEFAULT));
  const last = RANGE_PRESETS.some(([s]) => String(s) === stored) ? Number(stored) : DEFAULT;
  const set = useCallback((s: number) => store(String(s)), [store]);
  return <Ctx.Provider value={[last, set]}>{children}</Ctx.Provider>;
}

/** The project's time window in seconds, and its setter. */
export function useLastSeconds() {
  return useContext(Ctx);
}
