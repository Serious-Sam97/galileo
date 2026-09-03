"use client";

import { useMemo, useState } from "react";
import { Plus, X, Play, Save } from "lucide-react";
import { Button, Input, Select, Kbd } from "@/components/ui";
import { TimeRangePicker } from "@/components/time-range";
import { useFields } from "@/lib/hooks";
import { CALC_OPS, FILTER_OPS, NEEDS_FIELD, filterOpMeta, type Calculation, type Dataset, type Derived, type Filter, type Having, type Query } from "@/lib/types";
import { fmtDuration } from "@/lib/format";

function FieldInput({ dataset, value, onChange, placeholder = "field", listId }: { dataset: Dataset; value: string; onChange: (v: string) => void; placeholder?: string; listId: string }) {
  return <Input list={listId} value={value} onChange={(e) => onChange(e.target.value)} placeholder={placeholder} className="font-mono" />;
}

export function QueryBuilder({ query, onChange, onRun, onSave, running }: { query: Query; onChange: (q: Query) => void; onRun: () => void; onSave?: () => void; running?: boolean }) {
  const fields = useFields(query.dataset);
  const listId = `fields-${query.dataset}`;
  const [advanced, setAdvanced] = useState(false);
  const fieldNames = useMemo(() => fields.data?.fields.map((f) => f.name) ?? [], [fields.data]);
  const set = (patch: Partial<Query>) => onChange({ ...query, ...patch });
  const setCalc = (i: number, c: Calculation) => set({ calculations: query.calculations.map((x, j) => (j === i ? c : x)) });
  const setFilter = (i: number, f: Filter) => set({ filters: query.filters.map((x, j) => (j === i ? f : x)) });
  const setHaving = (i: number, h: Having) => set({ having: (query.having ?? []).map((x, j) => (j === i ? h : x)) });

  return (
    <div className="rounded-lg border bg-panel p-3 space-y-3" onKeyDown={(e) => { if ((e.metaKey || e.ctrlKey) && e.key === "Enter") onRun(); }}>
      <datalist id={listId}>{fieldNames.map((n) => <option key={n} value={n} />)}</datalist>
      <div className="flex flex-wrap items-center gap-2">
        <Select value={query.dataset} onChange={(e) => set({ dataset: e.target.value as Dataset, calculations: [{ op: "COUNT" }], filters: [], breakdowns: [], orders: [] })}>
          <option value="spans">Spans</option><option value="logs">Logs</option><option value="metrics">Metrics</option>
        </Select>
        <TimeRangePicker value={query.time_range} onChange={(t) => set({ time_range: t })} />
        <div className="ml-auto flex items-center gap-2">
          <label className="flex items-center gap-1 text-xs text-muted" title="Draw a dashed ghost line from the previous, equal-length window"><input type="checkbox" checked={!!query.compare_to} onChange={(e) => set({ compare_to: e.target.checked ? "previous" : null })} /> compare</label>
          <span className="text-xs text-muted hidden md:inline"><Kbd>⌘</Kbd> <Kbd>↵</Kbd> to run</span>
          {onSave && <Button size="sm" onClick={onSave}><Save size={13} /> Save</Button>}
          <Button size="sm" variant="primary" onClick={onRun} disabled={running}><Play size={13} /> {running ? "Running…" : "Run"}</Button>
        </div>
      </div>

      <div className="grid gap-3 md:grid-cols-2">
        <Section title="Visualize" onAdd={() => set({ calculations: [...query.calculations, { op: "COUNT" }] })} hint="empty = raw events">
          {query.calculations.map((c, i) => (
            <Row key={i} onRemove={() => set({ calculations: query.calculations.filter((_, j) => j !== i) })}>
              <Select value={c.op} onChange={(e) => setCalc(i, { ...c, op: e.target.value as Calculation["op"] })}>
                {CALC_OPS.map((o) => <option key={o} value={o}>{o}</option>)}
              </Select>
              {NEEDS_FIELD(c.op) && <FieldInput dataset={query.dataset} value={c.field ?? ""} onChange={(v) => setCalc(i, { ...c, field: v })} listId={listId} placeholder={query.dataset === "spans" ? "duration_ms" : "value"} />}
            </Row>
          ))}
        </Section>

        <Section title="Where" onAdd={() => set({ filters: [...query.filters, { field: "", op: "eq", value: "" }] })} hint={query.filters.length > 1 ? (
          <Select value={query.filter_combination ?? "AND"} onChange={(e) => set({ filter_combination: e.target.value as "AND" | "OR" })} className="py-0.5 text-xs"><option>AND</option><option>OR</option></Select>
        ) : undefined}>
          {query.filters.map((f, i) => {
            const meta = filterOpMeta(f.op);
            return (
              <Row key={i} onRemove={() => set({ filters: query.filters.filter((_, j) => j !== i) })}>
                <FieldInput dataset={query.dataset} value={f.field} onChange={(v) => setFilter(i, { ...f, field: v })} listId={listId} />
                <Select value={meta.v} onChange={(e) => setFilter(i, { ...f, op: e.target.value as Filter["op"] })}>
                  {FILTER_OPS.map((o) => <option key={o.v} value={o.v}>{o.label}</option>)}
                </Select>
                {meta.needsValue && (
                  <Input value={valueToText(f.value)} onChange={(e) => setFilter(i, { ...f, value: parseValue(e.target.value, f.op) })} placeholder={f.op === "in" || f.op === "not_in" ? "a, b, c" : "value"} className="font-mono" />
                )}
              </Row>
            );
          })}
        </Section>

        <Section title="Group by" onAdd={() => set({ breakdowns: [...query.breakdowns, ""] })}>
          {query.breakdowns.map((b, i) => (
            <Row key={i} onRemove={() => set({ breakdowns: query.breakdowns.filter((_, j) => j !== i) })}>
              <FieldInput dataset={query.dataset} value={b} onChange={(v) => set({ breakdowns: query.breakdowns.map((x, j) => (j === i ? v : x)) })} listId={listId} placeholder="service.name" />
            </Row>
          ))}
        </Section>

        <Section title="Derived columns" onAdd={() => set({ derived: [...(query.derived ?? []), { name: "", expr: "" }] })} hint="arithmetic on values">
          {(query.derived ?? []).map((d, i) => (
            <Row key={i} onRemove={() => set({ derived: (query.derived ?? []).filter((_, j) => j !== i) })}>
              <Input value={d.name} onChange={(e) => set({ derived: (query.derived ?? []).map((x, j) => (j === i ? { ...x, name: e.target.value } : x)) })} placeholder="name" className="w-28" />
              <span className="text-muted text-xs">=</span>
              <Input list={listId} value={d.expr} onChange={(e) => set({ derived: (query.derived ?? []).map((x, j) => (j === i ? { ...x, expr: e.target.value } : x)) })} placeholder="SUM(gen_ai.usage.cost_usd) / SUM(gen_ai.usage.output_tokens)" className="font-mono flex-1" />
            </Row>
          ))}
        </Section>

        <Section title="Having" onAdd={() => set({ having: [...(query.having ?? []), { target: query.calculations[0] ? calcLabel(query.calculations[0]) : "COUNT", op: "gt", value: 0 }] })} hint="filter groups">
          {(query.having ?? []).map((h, i) => (
            <Row key={i} onRemove={() => set({ having: (query.having ?? []).filter((_, j) => j !== i) })}>
              <Select value={h.target} onChange={(e) => setHaving(i, { ...h, target: e.target.value })}>
                {query.calculations.map((c) => { const l = calcLabel(c); return <option key={l} value={l}>{l}</option>; })}
                {(query.derived ?? []).filter((d) => d.name).map((d) => <option key={d.name} value={d.name}>{d.name}</option>)}
              </Select>
              <Select value={h.op} onChange={(e) => setHaving(i, { ...h, op: e.target.value as Having["op"] })}>{["gt", "gte", "lt", "lte", "eq", "ne"].map((o) => <option key={o} value={o}>{filterOpMeta(o as Having["op"]).label}</option>)}</Select>
              <Input type="number" value={h.value} onChange={(e) => setHaving(i, { ...h, value: Number(e.target.value) })} className="w-24 font-mono" />
            </Row>
          ))}
        </Section>

        <div className="space-y-2">
          <div className="flex items-center justify-between"><div className="text-[11px] uppercase tracking-wide text-muted">Order · Limit · Granularity</div>
            <button className="text-[11px] text-muted hover:text-fg" onClick={() => setAdvanced(!advanced)}>{advanced ? "less" : "more"}</button></div>
          <div className="flex flex-wrap gap-2">
            <Select value={query.orders[0]?.field ?? ""} onChange={(e) => set({ orders: e.target.value ? [{ field: e.target.value, direction: query.orders[0]?.direction ?? "desc" }] : [] })}>
              <option value="">default order</option>
              {query.calculations.map((c) => { const l = NEEDS_FIELD(c.op) ? `${c.op}(${c.field ?? ""})` : c.op; return <option key={l} value={l}>{l}</option>; })}
              {query.breakdowns.filter(Boolean).map((b) => <option key={b} value={b}>{b}</option>)}
            </Select>
            <Select value={query.orders[0]?.direction ?? "desc"} onChange={(e) => query.orders[0] && set({ orders: [{ ...query.orders[0], direction: e.target.value as "asc" | "desc" }] })}>
              <option value="desc">desc</option><option value="asc">asc</option>
            </Select>
            <Input type="number" className="w-24" value={query.limit ?? ""} onChange={(e) => set({ limit: e.target.value ? Number(e.target.value) : undefined })} placeholder="limit" />
            {advanced && (
              <>
                <Select value={query.granularity ?? ""} onChange={(e) => set({ granularity: e.target.value ? Number(e.target.value) : undefined })}>
                  <option value="">auto granularity</option>
                  {[10, 30, 60, 300, 900, 3600, 86400].map((g) => <option key={g} value={g}>{fmtDuration(g)} buckets</option>)}
                </Select>
                <Input className="flex-1 min-w-40" value={query.search ?? ""} onChange={(e) => set({ search: e.target.value || undefined })} placeholder={query.dataset === "logs" ? "full-text search in body" : "search in name / path"} />
              </>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

export function calcLabel(c: Calculation): string { return NEEDS_FIELD(c.op) ? `${c.op}(${c.field ?? ""})` : c.op; }

function Section({ title, onAdd, hint, children }: { title: string; onAdd: () => void; hint?: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <div className="text-[11px] uppercase tracking-wide text-muted flex items-center gap-2">{title} {typeof hint === "string" ? <span className="normal-case text-muted/60">({hint})</span> : hint}</div>
        <button className="text-muted hover:text-fg" onClick={onAdd} title="add"><Plus size={14} /></button>
      </div>
      {children}
    </div>
  );
}

function Row({ children, onRemove }: { children: React.ReactNode; onRemove: () => void }) {
  return (
    <div className="flex items-center gap-1.5">
      {children}
      <button className="text-muted hover:text-err shrink-0" onClick={onRemove}><X size={14} /></button>
    </div>
  );
}

export function valueToText(v: unknown): string {
  if (v === undefined || v === null) return "";
  if (Array.isArray(v)) return v.join(", ");
  return String(v);
}

export function parseValue(text: string, op: Filter["op"]): unknown {
  if (op === "in" || op === "not_in") return text.split(",").map((s) => s.trim()).filter(Boolean).map(coerce);
  return coerce(text);
}

function coerce(s: string): unknown {
  const t = s.trim();
  if (t === "") return "";
  if (/^-?\d+(\.\d+)?$/.test(t)) return Number(t);
  if (t === "true") return true;
  if (t === "false") return false;
  return t;
}
