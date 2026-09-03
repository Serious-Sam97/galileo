"use client";

import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { post } from "@/lib/api";
import { useProjectId, useProjectQuery } from "@/lib/hooks";
import { Button, Input, Label, Select, Textarea, ErrorBox, Badge } from "@/components/ui";
import { FilterEditor } from "@/components/query/filter-editor";
import { RecipientsEditor, type Recipient } from "@/components/recipients";
import { CALC_OPS, NEEDS_FIELD, type CalcOp, type Dataset, type Query, type Trigger } from "@/lib/types";
import { fmtNum } from "@/lib/format";

export interface TriggerDraft {
  name: string; description: string; query: Query; op: string; threshold: number; frequency_secs: number; window_secs: number; enabled: boolean; recipients: Recipient[];
  warn_threshold: number | null; for_secs: number; mute_hours: number | null; per_group: boolean; mode: "threshold" | "baseline" | "anomaly" | "outlier"; baseline_factor: number; baseline_min_delta: number; sensitivity: number; min_value: number; composite: { all_of?: string[]; any_of?: string[]; within_secs?: number } | null;
}

export const emptyTrigger = (): TriggerDraft => ({
  name: "", description: "", op: ">", threshold: 0, frequency_secs: 60, window_secs: 300, enabled: true, recipients: [],
  warn_threshold: null, for_secs: 0, mute_hours: null, per_group: false, mode: "threshold", baseline_factor: 2, baseline_min_delta: 0, sensitivity: 3, min_value: 0, composite: null,
  query: { dataset: "spans", time_range: { last_seconds: 300 }, calculations: [{ op: "COUNT" }], filters: [{ field: "is_error", op: "eq", value: 1 }], breakdowns: [], orders: [], limit: 50 },
});

export const draftOf = (t: Trigger): TriggerDraft => ({ name: t.name, description: t.description, query: t.query, op: t.op, threshold: t.threshold, frequency_secs: t.frequency_secs, window_secs: t.window_secs, enabled: t.enabled, recipients: t.recipients,
  warn_threshold: t.warn_threshold, for_secs: t.for_secs, mute_hours: null, per_group: t.per_group, mode: t.mode, baseline_factor: t.baseline_factor, baseline_min_delta: t.baseline_min_delta, sensitivity: t.sensitivity ?? 3, min_value: t.min_value ?? 0, composite: t.composite ?? null });

interface Evaluation { value: number | null; triggered: boolean; state: string; groups: [string[], number | null][]; error: string | null; severity: string; baseline: number | null; group_severities: [string, number | null, string][]; band?: number | null }

export function TriggerForm({ initial, onSubmit, error }: { initial: TriggerDraft; onSubmit: (d: TriggerDraft) => Promise<void>; error: unknown }) {
  const pid = useProjectId();
  const [d, setD] = useState<TriggerDraft>(initial);
  const preview = useMutation({ mutationFn: (b: TriggerDraft) => post<{ evaluation: Evaluation }>(`/api/projects/${pid}/triggers/preview`, b) });
  const calc = d.query.calculations[0] ?? { op: "COUNT" as CalcOp };
  const setQ = (patch: Partial<Query>) => setD({ ...d, query: { ...d.query, ...patch } });
  return (
    <form className="space-y-4" onSubmit={async (e) => { e.preventDefault(); await onSubmit(d); }}>
      <div className="grid grid-cols-2 gap-3">
        <div><Label>Name</Label><Input value={d.name} onChange={(e) => setD({ ...d, name: e.target.value })} required /></div>
        <div><Label>Description</Label><Input value={d.description} onChange={(e) => setD({ ...d, description: e.target.value })} /></div>
      </div>
      <div>
        <Label>Alert when</Label>
        <div className="flex flex-wrap items-center gap-1.5">
          <Select value={d.query.dataset} onChange={(e) => setQ({ dataset: e.target.value as Dataset, filters: [] })}><option value="spans">spans</option><option value="logs">logs</option><option value="metrics">metrics</option></Select>
          <Select value={calc.op} onChange={(e) => setQ({ calculations: [{ ...calc, op: e.target.value as CalcOp }] })}>{CALC_OPS.filter((o) => o !== "HEATMAP").map((o) => <option key={o}>{o}</option>)}</Select>
          {NEEDS_FIELD(calc.op) && <Input className="w-40 font-mono" value={calc.field ?? ""} onChange={(e) => setQ({ calculations: [{ ...calc, field: e.target.value }] })} placeholder="duration_ms" />}
          <Select value={d.op} onChange={(e) => setD({ ...d, op: e.target.value })}>{[">", ">=", "<", "<=", "=", "!="].map((o) => <option key={o}>{o}</option>)}</Select>
          {d.mode === "threshold" ? (
            <>
              <Input type="number" step="any" className="w-28" value={d.threshold} onChange={(e) => setD({ ...d, threshold: Number(e.target.value) })} title="critical threshold" />
              <span className="text-xs text-muted">critical · warn at</span>
              <Input type="number" step="any" className="w-24" value={d.warn_threshold ?? ""} onChange={(e) => setD({ ...d, warn_threshold: e.target.value === "" ? null : Number(e.target.value) })} placeholder="optional" />
            </>
          ) : (
            <>
              <Input type="number" step="0.1" className="w-20" value={d.baseline_factor} onChange={(e) => setD({ ...d, baseline_factor: Number(e.target.value) })} title="multiplier" /><span className="text-xs text-muted">× last week&apos;s value +</span>
              <Input type="number" step="any" className="w-20" value={d.baseline_min_delta} onChange={(e) => setD({ ...d, baseline_min_delta: Number(e.target.value) })} title="minimum delta" />
            </>
          )}
        </div>
        <div className="mt-1.5 flex flex-wrap items-center gap-3 text-xs">
          <label className="flex items-center gap-1"><input type="radio" checked={d.mode === "threshold"} onChange={() => setD({ ...d, mode: "threshold" })} /> fixed threshold</label>
          <label className="flex items-center gap-1"><input type="radio" checked={d.mode === "baseline"} onChange={() => setD({ ...d, mode: "baseline" })} /> baseline: same window 7 days ago</label>
          <label className="flex items-center gap-1"><input type="radio" checked={d.mode === "anomaly"} onChange={() => setD({ ...d, mode: "anomaly" })} /> anomaly: vs the same time of day, last 7 days</label>
          <label className="flex items-center gap-1"><input type="radio" checked={d.mode === "outlier"} onChange={() => setD({ ...d, mode: "outlier", per_group: true })} /> outlier: one group vs the others</label>
          {(d.mode === "anomaly" || d.mode === "outlier") && <label className="flex items-center gap-1">sensitivity <Input type="number" min={1} max={5} className="w-14 py-0.5" value={d.sensitivity} onChange={(e) => setD({ ...d, sensitivity: Number(e.target.value) })} /> /5</label>}
          {d.mode === "anomaly" && <label className="flex items-center gap-1">ignore below <Input type="number" step="any" className="w-20 py-0.5" value={d.min_value} onChange={(e) => setD({ ...d, min_value: Number(e.target.value) })} /></label>}
          <span className="text-muted">·</span>
          <label className="flex items-center gap-1"><input type="checkbox" checked={!!d.composite} onChange={(e) => setD({ ...d, composite: e.target.checked ? { all_of: [], within_secs: 600 } : null })} /> composite of other triggers</label>
          {d.composite && <CompositeEditor value={d.composite} onChange={(c) => setD({ ...d, composite: c })} />}
          <label className="flex items-center gap-1">sustained for <Input type="number" min={0} className="w-16 py-0.5" value={Math.round(d.for_secs / 60)} onChange={(e) => setD({ ...d, for_secs: Number(e.target.value) * 60 })} /> min</label>
          <label className="flex items-center gap-1"><input type="checkbox" checked={d.per_group} onChange={(e) => setD({ ...d, per_group: e.target.checked })} /> alert per group</label>
          <label className="flex items-center gap-1">mute for <Input type="number" min={0} step="0.5" className="w-16 py-0.5" value={d.mute_hours ?? ""} onChange={(e) => setD({ ...d, mute_hours: e.target.value === "" ? null : Number(e.target.value) })} placeholder="0" /> h</label>
        </div>
      </div>
      <div><Label>Where</Label><FilterEditor dataset={d.query.dataset} filters={d.query.filters} onChange={(f) => setQ({ filters: f })} listId="trigger-fields" /></div>
      <div><Label>Group by (worst group decides, comma separated)</Label><Input className="font-mono" value={d.query.breakdowns.join(", ")} onChange={(e) => setQ({ breakdowns: e.target.value.split(",").map((s) => s.trim()).filter(Boolean) })} placeholder="service.name" /></div>
      <div className="grid grid-cols-3 gap-3">
        <div><Label>Window (s)</Label><Input type="number" min={30} value={d.window_secs} onChange={(e) => setD({ ...d, window_secs: Number(e.target.value) })} /></div>
        <div><Label>Evaluate every (s)</Label><Input type="number" min={30} value={d.frequency_secs} onChange={(e) => setD({ ...d, frequency_secs: Number(e.target.value) })} /></div>
        <div><Label>Enabled</Label><label className="flex items-center gap-2 py-1.5"><input type="checkbox" checked={d.enabled} onChange={(e) => setD({ ...d, enabled: e.target.checked })} /> on</label></div>
      </div>
      <div><Label>Notify</Label><RecipientsEditor value={d.recipients} onChange={(r) => setD({ ...d, recipients: r })} /></div>
      <div className="rounded-md border bg-bg p-2">
        <div className="flex items-center gap-2"><Button type="button" size="sm" onClick={() => preview.mutate(d)} disabled={preview.isPending}>Preview now</Button>
          {preview.data && <span className="text-sm">{preview.data.evaluation.error ? <span className="text-err">{preview.data.evaluation.error}</span> : <><Badge tone={preview.data.evaluation.severity === "critical" ? "err" : preview.data.evaluation.severity === "warn" ? "warn" : "ok"}>{preview.data.evaluation.severity}</Badge> value = <b className="font-mono">{fmtNum(preview.data.evaluation.value)}</b>{preview.data.evaluation.baseline != null && <span className="text-muted"> · baseline {fmtNum(preview.data.evaluation.baseline)}{preview.data.evaluation.band != null ? ` ± ${fmtNum(preview.data.evaluation.band)}` : ""}</span>}</>}</span>}</div>
        {preview.data && preview.data.evaluation.groups.length > 1 && <div className="mt-1 text-[11px] text-muted font-mono">{preview.data.evaluation.groups.slice(0, 8).map(([k, v]) => `${k.join(",")}=${fmtNum(v)}`).join("  ")}</div>}
        <ErrorBox error={preview.error} />
      </div>
      <ErrorBox error={error} />
      <Button type="submit" variant="primary">Save trigger</Button>
      <Textarea className="hidden" readOnly value={JSON.stringify(d.query)} />
    </form>
  );
}

function CompositeEditor({ value, onChange }: { value: { all_of?: string[]; any_of?: string[]; within_secs?: number }; onChange: (c: { all_of?: string[]; any_of?: string[]; within_secs?: number }) => void }) {
  const list = useProjectQuery<{ triggers: { id: string; name: string }[] }>(["triggers"], "/triggers");
  const mode: "all_of" | "any_of" = value.any_of ? "any_of" : "all_of";
  const ids = value[mode] ?? [];
  const set = (m: "all_of" | "any_of", next: string[]) => onChange({ [m]: next, within_secs: value.within_secs ?? 600 });
  return (
    <div className="flex flex-wrap items-center gap-2 rounded border p-2 text-xs w-full">
      <Select value={mode} onChange={(e) => set(e.target.value as "all_of" | "any_of", ids)}><option value="all_of">all of</option><option value="any_of">any of</option></Select>
      {(list.data?.triggers ?? []).map((t: { id: string; name: string }) => <label key={t.id} className="flex items-center gap-1"><input type="checkbox" checked={ids.includes(t.id)} onChange={(e) => set(mode, e.target.checked ? [...ids, t.id] : ids.filter((x) => x !== t.id))} /> {t.name}</label>)}
      <label className="flex items-center gap-1">within <Input type="number" className="w-20 py-0.5" value={value.within_secs ?? 600} onChange={(e) => onChange({ ...value, within_secs: Number(e.target.value) })} /> s</label>
      <span className="text-muted">The trigger&apos;s own query is ignored; it fires when the members do.</span>
    </div>
  );
}
