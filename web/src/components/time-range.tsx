"use client";

import { Select } from "./ui";
import type { TimeRange } from "@/lib/types";
import { fmtDuration } from "@/lib/format";

const PRESETS = [
  [900, "Last 15m"], [3600, "Last 1h"], [4 * 3600, "Last 4h"], [24 * 3600, "Last 24h"], [3 * 86400, "Last 3d"], [7 * 86400, "Last 7d"], [30 * 86400, "Last 30d"],
] as const;

export function TimeRangePicker({ value, onChange }: { value: TimeRange; onChange: (t: TimeRange) => void }) {
  const cur = "last_seconds" in value ? String(value.last_seconds) : "custom";
  const known = PRESETS.some(([s]) => String(s) === cur);
  return (
    <div className="flex items-center gap-2">
      <Select value={cur} onChange={(e) => e.target.value !== "custom" && onChange({ last_seconds: Number(e.target.value) })}>
        {PRESETS.map(([s, l]) => <option key={s} value={s}>{l}</option>)}
        {!known && cur !== "custom" && <option value={cur}>Last {fmtDuration(Number(cur))}</option>}
        {cur === "custom" && <option value="custom">Custom</option>}
      </Select>
      {"start" in value && (
        <span className="text-xs text-muted font-mono">
          {new Date(value.start).toLocaleString()} → {new Date(value.end).toLocaleString()}
        </span>
      )}
    </div>
  );
}
