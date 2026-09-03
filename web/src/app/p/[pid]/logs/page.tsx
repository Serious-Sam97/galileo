"use client";

import { useState } from "react";
import clsx from "clsx";
import Link from "next/link";
import { useProjectId, useRunQuery } from "@/lib/hooks";
import { Button, Input, Select, Empty, ErrorBox, Badge, Drawer } from "@/components/ui";
import { TimeRangePicker } from "@/components/time-range";
import { SeriesCharts } from "@/components/query/result-charts";
import type { Query, TimeRange } from "@/lib/types";
import { fmtTime } from "@/lib/format";
import { Radio } from "lucide-react";

const SEV_TONE: Record<string, "err" | "warn" | "ok" | "muted" | "info"> = { fatal: "err", error: "err", warn: "warn", info: "ok", debug: "muted", trace: "muted" };

export default function LogsPage() {
  const pid = useProjectId();
  const [range, setRange] = useState<TimeRange>({ last_seconds: 3600 });
  const [search, setSearch] = useState("");
  const [severity, setSeverity] = useState("");
  const [service, setService] = useState("");
  const [user, setUser] = useState("");
  const [live, setLive] = useState(false);
  const [applied, setApplied] = useState({ search: "", severity: "", service: "", user: "" });
  const [selected, setSelected] = useState<Record<string, unknown> | null>(null);

  const filters: Query["filters"] = [];
  if (applied.severity) filters.push({ field: "severity", op: "eq", value: applied.severity });
  if (applied.service) filters.push({ field: "service.name", op: "eq", value: applied.service });
  if (applied.user) filters.push({ field: "user.id", op: "eq", value: applied.user });
  const base: Query = { dataset: "logs", time_range: live ? { last_seconds: 300 } : range, calculations: [], filters, breakdowns: [], orders: [], limit: 300, search: applied.search || undefined, columns: ["scope_name"] };
  const rows = useRunQuery(base, { refetchInterval: live ? 3000 : undefined });
  const hist = useRunQuery({ ...base, calculations: [{ op: "COUNT" }], breakdowns: ["severity"], limit: 10 }, { refetchInterval: live ? 5000 : undefined });
  const apply = () => setApplied({ search, severity, service, user });

  const cols = rows.data?.raw?.columns ?? [];
  const ix = (c: string) => cols.indexOf(c);

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2 rounded-lg border bg-panel p-3">
        <TimeRangePicker value={range} onChange={setRange} />
        <Input className="flex-1 min-w-60" placeholder="search log bodies (all words must match)" value={search} onChange={(e) => setSearch(e.target.value)} onKeyDown={(e) => e.key === "Enter" && apply()} />
        <Select value={severity} onChange={(e) => setSeverity(e.target.value)}><option value="">any level</option>{["fatal", "error", "warn", "info", "debug", "trace"].map((s) => <option key={s}>{s}</option>)}</Select>
        <Input className="w-36" placeholder="service" value={service} onChange={(e) => setService(e.target.value)} />
        <Input className="w-28" placeholder="user id" value={user} onChange={(e) => setUser(e.target.value)} />
        <Button variant="primary" size="sm" onClick={apply}>Search</Button>
        <Button size="sm" variant={live ? "primary" : "outline"} onClick={() => setLive(!live)} title="poll every 3s for the last 5 minutes"><Radio size={13} /> {live ? "Live" : "Tail"}</Button>
      </div>
      <ErrorBox error={rows.error} />
      {hist.data && hist.data.groups.length > 0 && <SeriesCharts res={hist.data} />}
      {rows.data?.raw && (rows.data.raw.rows.length === 0 ? <Empty>No log records match.</Empty> : (
        <div className="rounded-md border overflow-auto scroll-thin max-h-[calc(100vh-380px)] font-mono text-[12px]">
          {rows.data.raw.rows.map((r, i) => {
            const sev = String(r[ix("severity")]);
            const obj = Object.fromEntries(cols.map((c, j) => [c, r[j]]));
            return (
              <div key={i} className="flex gap-2 border-b border-border/50 px-2 py-1 hover:bg-panel-2 cursor-pointer" onClick={() => setSelected(obj)}>
                <span className="text-muted whitespace-nowrap">{fmtTime(String(r[ix("timestamp")]))}</span>
                <Badge tone={SEV_TONE[sev] ?? "muted"} className="w-12 justify-center">{sev}</Badge>
                <span className="text-muted w-20 truncate shrink-0">{String(r[ix("service_name")])}</span>
                {r[ix("user_id")] ? <Link href={`/p/${pid}/users/${r[ix("user_id")]}`} onClick={(e) => e.stopPropagation()} className="w-14 shrink-0 truncate text-info hover:underline" title={`user ${r[ix("user_id")]}`}>u:{String(r[ix("user_id")])}</Link> : <span className="w-14 shrink-0 text-muted/50">–</span>}
                {r[ix("tenant_id")] ? <span className="w-10 shrink-0 truncate text-muted" title={`tenant ${r[ix("tenant_id")]}`}>t:{String(r[ix("tenant_id")])}</span> : <span className="w-10 shrink-0" />}
                {r[ix("code_function")] ? <span className="w-28 shrink-0 truncate text-accent/80" title={String(r[ix("code_function")])}>{String(r[ix("code_function")])}()</span> : null}
                <span className={clsx("whitespace-pre-wrap break-all", sev === "error" || sev === "fatal" ? "text-err" : "")}>{String(r[ix("body")])}</span>
                {r[ix("trace_id")] ? <Link href={`/p/${pid}/traces/${r[ix("trace_id")]}`} onClick={(e) => e.stopPropagation()} className="ml-auto text-info hover:underline shrink-0">trace</Link> : null}
              </div>
            );
          })}
        </div>
      ))}
      <Drawer open={!!selected} onClose={() => setSelected(null)} title="Log record">
        {selected && (
          <div className="space-y-3 text-sm">
            <pre className="whitespace-pre-wrap rounded border bg-bg p-2 font-mono text-[12px]">{String(selected.body)}</pre>
            <div className="grid gap-x-3 gap-y-1 text-[12px]" style={{ gridTemplateColumns: "max-content 1fr" }}>
              {Object.entries(selected).filter(([k]) => k !== "body" && k !== "attrs").map(([k, v]) => <div key={k} className="contents"><div className="font-mono text-muted">{k}</div><div className="font-mono break-all">{String(v ?? "")}</div></div>)}
              {!!selected.attrs && typeof selected.attrs === "object" && Object.entries(selected.attrs as Record<string, string>).map(([k, v]) => <div key={k} className="contents"><div className="font-mono text-muted">{k}</div><div className="font-mono break-all">{v}</div></div>)}
            </div>
            {selected.trace_id ? <Link href={`/p/${pid}/traces/${selected.trace_id}`} className="text-info hover:underline">Open trace →</Link> : null}
          </div>
        )}
      </Drawer>
    </div>
  );
}
