"use client";

import { useMutation } from "@tanstack/react-query";
import { useEffect } from "react";
import { post } from "@/lib/api";
import { useProjectId } from "@/lib/hooks";
import type { BubbleUpResponse, Filter, Query } from "@/lib/types";
import { Card, ErrorBox, Button, Input, Select } from "@/components/ui";
import { fmtNum } from "@/lib/format";
import { useState } from "react";
import { FILTER_OPS } from "@/lib/types";
import { parseValue } from "./builder";

/** Start a BubbleUp from a typed condition (the mouse brush on charts does the same). */
export function BubbleUpBar({ defaultField, onSelect }: { defaultField: string; onSelect: (f: Filter) => void }) {
  const [field, setField] = useState(defaultField);
  const [op, setOp] = useState<Filter["op"]>("gt");
  const [value, setValue] = useState("");
  return (
    <form className="flex flex-wrap items-center gap-1.5 rounded-lg border bg-panel px-3 py-2 text-xs" onSubmit={(e) => { e.preventDefault(); if (field) onSelect({ field, op, value: parseValue(value, op) }); }}>
      <span className="text-muted mr-1">BubbleUp: compare events where</span>
      <Input className="w-40 font-mono" value={field} onChange={(e) => setField(e.target.value)} placeholder="duration_ms" />
      <Select value={op} onChange={(e) => setOp(e.target.value as Filter["op"])}>{FILTER_OPS.filter((o) => o.needsValue).map((o) => <option key={o.v} value={o.v}>{o.label}</option>)}</Select>
      <Input className="w-32 font-mono" value={value} onChange={(e) => setValue(e.target.value)} placeholder="500" />
      <Button type="submit" size="sm" variant="primary">Compare</Button>
      <span className="text-muted ml-2">or drag-select on a chart</span>
    </form>
  );
}

export function BubbleUp({ query, selection, onClear, onAddFilter }: { query: Query; selection: Filter[]; onClear: () => void; onAddFilter: (f: Filter) => void }) {
  const pid = useProjectId();
  const m = useMutation({ mutationFn: (sel: Filter[]) => post<BubbleUpResponse>(`/api/projects/${pid}/bubbleup`, { query, selection: sel, max_keys: 24, max_values: 6 }) });
  const { mutate } = m;
  useEffect(() => { if (selection.length) mutate(selection); }, [selection, mutate]);
  if (!selection.length) return null;
  const d = m.data;
  return (
    <Card title={<span>BubbleUp — {selection.map((f) => `${f.field} ${f.op} ${String(f.value)}`).join(" AND ")}</span>} actions={<Button size="sm" variant="ghost" onClick={onClear}>clear</Button>}>
      <ErrorBox error={m.error} />
      {m.isPending && <div className="text-muted text-sm">Comparing attribute distributions…</div>}
      {d && (
        <>
          <div className="text-xs text-muted mb-3">
            <b className="text-fg">{fmtNum(d.inside_count)}</b> events inside the selection vs <b className="text-fg">{fmtNum(d.outside_count)}</b> outside. Bars: share of events inside (amber) vs outside (grey). Sorted by how different the distributions are.
          </div>
          <div className="grid gap-3" style={{ gridTemplateColumns: "repeat(auto-fill, minmax(260px, 1fr))" }}>
            {d.keys.map((k) => (
              <div key={k.key} className="rounded-md border bg-bg p-2">
                <div className="flex items-center justify-between mb-1.5">
                  <span className="font-mono text-xs truncate" title={k.key}>{k.key}</span>
                  <span className="text-[10px] text-muted">score {(k.score * 100).toFixed(0)}</span>
                </div>
                <div className="space-y-1">
                  {k.values.map((v) => (
                    <button key={v.value} className="block w-full text-left group" title="add as filter" onClick={() => onAddFilter({ field: k.key, op: "eq", value: v.value })}>
                      <div className="flex justify-between text-[11px]"><span className="truncate font-mono group-hover:text-accent">{v.value}</span><span className="text-muted tabular-nums">{(v.inside_pct * 100).toFixed(0)}% / {(v.outside_pct * 100).toFixed(0)}%</span></div>
                      <div className="h-1.5 rounded bg-panel-2 relative overflow-hidden">
                        <div className="absolute inset-y-0 left-0 bg-muted/40" style={{ width: `${v.outside_pct * 100}%` }} />
                        <div className="absolute inset-y-0 left-0 bg-accent/80" style={{ width: `${v.inside_pct * 100}%` }} />
                      </div>
                    </button>
                  ))}
                </div>
              </div>
            ))}
          </div>
        </>
      )}
    </Card>
  );
}
