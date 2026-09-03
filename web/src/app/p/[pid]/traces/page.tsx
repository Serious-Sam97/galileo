"use client";

import Link from "next/link";
import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import { post } from "@/lib/api";
import { useProjectId } from "@/lib/hooks";
import { Button, Input, Select, Table, Th, Td, Empty, Badge, ErrorBox } from "@/components/ui";
import { TimeRangePicker } from "@/components/time-range";
import type { Query, QueryResponse, TimeRange } from "@/lib/types";
import { fmtMs, fmtTime } from "@/lib/format";
import { useEffect } from "react";

export default function TracesPage() {
  const pid = useProjectId();
  const [range, setRange] = useState<TimeRange>({ last_seconds: 3600 });
  const [service, setService] = useState("");
  const [status, setStatus] = useState("");
  const [search, setSearch] = useState("");
  const [minMs, setMinMs] = useState("");
  const [traceId, setTraceId] = useState("");
  const m = useMutation({ mutationFn: (q: Query) => post<QueryResponse>(`/api/projects/${pid}/traces`, q) });
  const { mutate } = m;

  const run = () => {
    const q: Query = { dataset: "spans", time_range: range, calculations: [], filters: [], breakdowns: [], orders: [], limit: 100, search: search || undefined };
    if (service) q.filters.push({ field: "service.name", op: "eq", value: service });
    if (status) q.filters.push({ field: "status_code", op: "eq", value: status });
    if (minMs) q.filters.push({ field: "duration_ms", op: "gte", value: Number(minMs) });
    mutate(q);
  };
  useEffect(() => { mutate({ dataset: "spans", time_range: { last_seconds: 3600 }, calculations: [], filters: [], breakdowns: [], orders: [], limit: 100 }); }, [mutate]);

  const cols = m.data?.raw?.columns ?? [];
  const ix = (c: string) => cols.indexOf(c);

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2 rounded-lg border bg-panel p-3">
        <TimeRangePicker value={range} onChange={setRange} />
        <Input className="w-40" placeholder="service" value={service} onChange={(e) => setService(e.target.value)} />
        <Select value={status} onChange={(e) => setStatus(e.target.value)}><option value="">any status</option><option value="error">error</option><option value="ok">ok</option><option value="unset">unset</option></Select>
        <Input className="w-28" placeholder="min ms" type="number" value={minMs} onChange={(e) => setMinMs(e.target.value)} />
        <Input data-page-filter className="w-48" placeholder="search name / path" value={search} onChange={(e) => setSearch(e.target.value)} onKeyDown={(e) => e.key === "Enter" && run()} />
        <Button variant="primary" size="sm" onClick={run}>Search</Button>
        <form className="ml-auto flex gap-1" onSubmit={(e) => { e.preventDefault(); if (traceId.trim()) window.location.href = `/p/${pid}/traces/${traceId.trim()}`; }}>
          <Input className="w-72 font-mono" placeholder="jump to trace id" value={traceId} onChange={(e) => setTraceId(e.target.value)} />
        </form>
      </div>
      <ErrorBox error={m.error} />
      {m.data?.raw && (m.data.raw.rows.length === 0 ? <Empty>No traces match.</Empty> : (
        <Table className="max-h-[calc(100vh-200px)]">
          <thead><tr><Th>Time</Th><Th>Service</Th><Th>Root span</Th><Th>Route</Th><Th className="text-right">Status</Th><Th className="text-right">Duration</Th><Th>User</Th><Th>Trace</Th></tr></thead>
          <tbody>
            {m.data.raw.rows.map((r, i) => {
              const st = String(r[ix("status_code")]);
              const http = r[ix("http_status_code")] as number;
              return (
                <tr key={i} data-row className="hover:bg-panel-2">
                  <Td className="whitespace-nowrap text-muted">{fmtTime(String(r[ix("timestamp")]))}</Td>
                  <Td>{String(r[ix("service_name")])}</Td>
                  <Td><Link href={`/p/${pid}/traces/${r[ix("trace_id")]}`} className="hover:text-accent font-medium">{String(r[ix("name")])}</Link></Td>
                  <Td className="font-mono text-muted">{String(r[ix("http_route")] ?? "")}</Td>
                  <Td className="text-right"><Badge tone={st === "error" ? "err" : st === "ok" ? "ok" : "muted"}>{http ? http : st}</Badge></Td>
                  <Td className="text-right font-mono tabular-nums">{fmtMs(Number(r[ix("duration_ms")]))}</Td>
                  <Td className="font-mono">{r[ix("user_id")] ? <Link href={`/p/${pid}/users/${r[ix("user_id")]}`} className="text-info hover:underline">{String(r[ix("user_id")])}</Link> : <span className="text-muted">–</span>}</Td>
                  <Td><Link href={`/p/${pid}/traces/${r[ix("trace_id")]}`} className="font-mono text-info hover:underline">{String(r[ix("trace_id")]).slice(0, 12)}…</Link></Td>
                </tr>
              );
            })}
          </tbody>
        </Table>
      ))}
    </div>
  );
}
