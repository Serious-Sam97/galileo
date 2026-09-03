"use client";

import { useState } from "react";
import { Button, Input, Label, Select, ErrorBox } from "@/components/ui";
import { FilterEditor } from "@/components/query/filter-editor";
import { RecipientsEditor, type Recipient } from "@/components/recipients";
import type { Dataset, Filter, Slo } from "@/lib/types";
import { Plus, X } from "lucide-react";

export interface SloDraft { name: string; description: string; dataset: Dataset; total_filters: Filter[]; good_filters: Filter[]; target_pct: number; window_days: number; burn_alerts: { name: string; long_window_mins: number; short_window_mins: number; burn_rate: number; recipients: Recipient[] }[] }

export const emptySlo = (): SloDraft => ({
  name: "", description: "", dataset: "spans", target_pct: 99.9, window_days: 30,
  total_filters: [{ field: "is_root", op: "eq", value: 1 }], good_filters: [{ field: "is_error", op: "eq", value: 0 }],
  burn_alerts: [{ name: "fast burn", long_window_mins: 60, short_window_mins: 5, burn_rate: 14.4, recipients: [] }, { name: "slow burn", long_window_mins: 360, short_window_mins: 30, burn_rate: 6, recipients: [] }],
});

export const draftOfSlo = (s: Slo): SloDraft => ({ name: s.name, description: s.description, dataset: s.dataset, total_filters: s.total_filters, good_filters: s.good_filters, target_pct: s.target_pct, window_days: s.window_days, burn_alerts: s.burn_alerts as SloDraft["burn_alerts"] });

export function SloForm({ initial, onSubmit, error }: { initial: SloDraft; onSubmit: (d: SloDraft) => Promise<void>; error: unknown }) {
  const [d, setD] = useState(initial);
  const setAlert = (i: number, patch: Partial<SloDraft["burn_alerts"][number]>) => setD({ ...d, burn_alerts: d.burn_alerts.map((a, j) => (j === i ? { ...a, ...patch } : a)) });
  return (
    <form className="space-y-4" onSubmit={async (e) => { e.preventDefault(); await onSubmit(d); }}>
      <div className="grid grid-cols-2 gap-3">
        <div><Label>Name</Label><Input value={d.name} onChange={(e) => setD({ ...d, name: e.target.value })} required /></div>
        <div><Label>Description</Label><Input value={d.description} onChange={(e) => setD({ ...d, description: e.target.value })} /></div>
      </div>
      <div className="grid grid-cols-3 gap-3">
        <div><Label>Dataset</Label><Select className="w-full" value={d.dataset} onChange={(e) => setD({ ...d, dataset: e.target.value as Dataset })}><option value="spans">spans</option><option value="logs">logs</option><option value="metrics">metrics</option></Select></div>
        <div><Label>Target %</Label><Input type="number" step="0.001" min={0} max={100} value={d.target_pct} onChange={(e) => setD({ ...d, target_pct: Number(e.target.value) })} /></div>
        <div><Label>Window (days)</Label><Input type="number" min={1} max={90} value={d.window_days} onChange={(e) => setD({ ...d, window_days: Number(e.target.value) })} /></div>
      </div>
      <div><Label>Total events (what counts)</Label><FilterEditor dataset={d.dataset} filters={d.total_filters} onChange={(f) => setD({ ...d, total_filters: f })} listId="slo-total" /></div>
      <div><Label>Good events (subset of total)</Label><FilterEditor dataset={d.dataset} filters={d.good_filters} onChange={(f) => setD({ ...d, good_filters: f })} listId="slo-good" /></div>
      <div>
        <Label>Burn-rate alerts (fire when both windows burn faster than the rate)</Label>
        {d.burn_alerts.map((a, i) => (
          <div key={i} className="mb-2 rounded border bg-bg p-2 space-y-2">
            <div className="flex gap-1.5 items-center">
              <Input className="w-32" value={a.name} onChange={(e) => setAlert(i, { name: e.target.value })} placeholder="name" />
              <span className="text-xs text-muted">long</span><Input type="number" className="w-20" value={a.long_window_mins} onChange={(e) => setAlert(i, { long_window_mins: Number(e.target.value) })} /><span className="text-xs text-muted">min · short</span>
              <Input type="number" className="w-20" value={a.short_window_mins} onChange={(e) => setAlert(i, { short_window_mins: Number(e.target.value) })} /><span className="text-xs text-muted">min · rate ×</span>
              <Input type="number" step="0.1" className="w-20" value={a.burn_rate} onChange={(e) => setAlert(i, { burn_rate: Number(e.target.value) })} />
              <button type="button" className="ml-auto text-muted hover:text-err" onClick={() => setD({ ...d, burn_alerts: d.burn_alerts.filter((_, j) => j !== i) })}><X size={14} /></button>
            </div>
            <RecipientsEditor value={a.recipients} onChange={(r) => setAlert(i, { recipients: r })} />
          </div>
        ))}
        <button type="button" className="flex items-center gap-1 text-xs text-muted hover:text-fg" onClick={() => setD({ ...d, burn_alerts: [...d.burn_alerts, { name: "", long_window_mins: 60, short_window_mins: 5, burn_rate: 14.4, recipients: [] }] })}><Plus size={12} /> burn alert</button>
      </div>
      <ErrorBox error={error} />
      <Button type="submit" variant="primary">Save SLO</Button>
    </form>
  );
}
