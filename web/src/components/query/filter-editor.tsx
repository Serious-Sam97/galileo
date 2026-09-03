"use client";

import { Plus, X } from "lucide-react";
import { Input, Select } from "@/components/ui";
import { FILTER_OPS, filterOpMeta, type Dataset, type Filter } from "@/lib/types";
import { useFields } from "@/lib/hooks";
import { parseValue, valueToText } from "./builder";

/** Compact filter list editor used by SLOs and triggers. */
export function FilterEditor({ dataset, filters, onChange, listId }: { dataset: Dataset; filters: Filter[]; onChange: (f: Filter[]) => void; listId: string }) {
  const fields = useFields(dataset);
  const setF = (i: number, f: Filter) => onChange(filters.map((x, j) => (j === i ? f : x)));
  return (
    <div className="space-y-1.5">
      <datalist id={listId}>{fields.data?.fields.map((f) => <option key={f.name} value={f.name} />)}</datalist>
      {filters.map((f, i) => {
        const meta = filterOpMeta(f.op);
        return (
          <div key={i} className="flex items-center gap-1.5">
            <Input list={listId} className="font-mono" value={f.field} onChange={(e) => setF(i, { ...f, field: e.target.value })} placeholder="field" />
            <Select value={meta.v} onChange={(e) => setF(i, { ...f, op: e.target.value as Filter["op"] })}>{FILTER_OPS.map((o) => <option key={o.v} value={o.v}>{o.label}</option>)}</Select>
            {meta.needsValue && <Input className="font-mono" value={valueToText(f.value)} onChange={(e) => setF(i, { ...f, value: parseValue(e.target.value, f.op) })} placeholder="value" />}
            <button type="button" className="text-muted hover:text-err" onClick={() => onChange(filters.filter((_, j) => j !== i))}><X size={14} /></button>
          </div>
        );
      })}
      <button type="button" className="flex items-center gap-1 text-xs text-muted hover:text-fg" onClick={() => onChange([...filters, { field: "", op: "eq", value: "" }])}><Plus size={12} /> filter</button>
    </div>
  );
}
