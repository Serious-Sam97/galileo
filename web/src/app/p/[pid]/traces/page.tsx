"use client";

import Link from "next/link";
import { useCallback, useEffect, useState } from "react";
import { useSearchParams } from "next/navigation";
import { useMutation } from "@tanstack/react-query";
import { post } from "@/lib/api";
import { useProjectId } from "@/lib/hooks";
import { Button, Input, Select, Table, Th, Td, Empty, Badge, ErrorBox, PageHeader, Skeleton } from "@/components/ui";
import type { Query, QueryResponse } from "@/lib/types";
import { fmtMs, fmtTime, fmtDuration } from "@/lib/format";
import { useLastSeconds } from "@/lib/time-range";
import { useT } from "@/lib/i18n";

export default function TracesPage() {
  const pid = useProjectId();
  const t = useT();
  const params = useSearchParams();
  const [last] = useLastSeconds();
  const [service, setService] = useState("");
  const [route, setRoute] = useState(params.get("route") ?? "");
  // From the Overview's "(no route)" rows: requests nothing matched, optionally of one method.
  const noRoute = params.get("noroute") === "1";
  const method = params.get("method") ?? "";
  const [status, setStatus] = useState("");
  const [search, setSearch] = useState("");
  const [minMs, setMinMs] = useState("");
  const [traceId, setTraceId] = useState("");
  const m = useMutation({ mutationFn: (q: Query) => post<QueryResponse>(`/api/projects/${pid}/traces`, q) });
  const { mutate } = m;

  const build = useCallback((): Query => {
    const q: Query = { dataset: "spans", time_range: { last_seconds: last }, calculations: [], filters: [], breakdowns: [], orders: [], limit: 100, search: search || undefined };
    if (service) q.filters.push({ field: "service.name", op: "eq", value: service });
    if (route) q.filters.push({ field: "http_route", op: "eq", value: route });
    else if (noRoute) q.filters.push({ field: "http_route", op: "eq", value: "" });
    if (method) q.filters.push({ field: "http_method", op: "eq", value: method });
    if (status) q.filters.push({ field: "status_code", op: "eq", value: status });
    if (minMs) q.filters.push({ field: "duration_ms", op: "gte", value: Number(minMs) });
    return q;
  }, [last, search, service, route, noRoute, method, status, minMs]);
  const run = () => mutate(build());
  // Re-run when the window changes (top bar); text filters run on Enter or Search.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => { mutate(build()); }, [mutate, last]);

  const cols = m.data?.raw?.columns ?? [];
  const ix = (c: string) => cols.indexOf(c);
  const rows = m.data?.raw?.rows ?? [];
  const maxMs = Math.max(1, ...rows.map((r) => Number(r[ix("duration_ms")]) || 0));

  return (
    <div className="mx-auto max-w-[1400px] space-y-4">
      <PageHeader title={t("Traces")} sub={`Root spans · ${fmtDuration(last)}${method ? ` · ${method}` : ""}${noRoute && !route ? " · no route matched" : ""}`} actions={
        <form className="flex gap-1" onSubmit={(e) => { e.preventDefault(); if (traceId.trim()) window.location.href = `/p/${pid}/traces/${traceId.trim()}`; }}>
          <Input className="w-72 font-mono" placeholder="jump to trace id" aria-label="Trace id" value={traceId} onChange={(e) => setTraceId(e.target.value)} />
        </form>
      } />
      <div className="flex flex-wrap items-center gap-2 rounded-xl border bg-panel/80 p-3" onKeyDown={(e) => e.key === "Enter" && run()}>
        <Input className="w-40" placeholder="service" aria-label="Service" value={service} onChange={(e) => setService(e.target.value)} />
        <Input className="w-56 font-mono" placeholder="route, e.g. /api/orders/{id}" aria-label="Route" value={route} onChange={(e) => setRoute(e.target.value)} />
        <Select value={status} aria-label="Status" onChange={(e) => setStatus(e.target.value)}><option value="">any status</option><option value="error">error</option><option value="ok">ok</option><option value="unset">unset</option></Select>
        <Input className="w-28" placeholder="min ms" aria-label="Minimum duration in ms" type="number" value={minMs} onChange={(e) => setMinMs(e.target.value)} />
        <Input data-page-filter className="w-48" placeholder="search name / path" aria-label="Search" value={search} onChange={(e) => setSearch(e.target.value)} />
        <Button variant="primary" size="sm" onClick={run}>Search</Button>
      </div>
      <ErrorBox error={m.error} />
      {m.isPending && !m.data ? <div className="space-y-2">{[0, 1, 2, 3, 4, 5].map((i) => <Skeleton key={i} className="h-9" />)}</div> : m.data?.raw && (rows.length === 0 ? <Empty>No traces match. Widen the time range in the top bar or clear a filter.</Empty> : (
        <Table className="max-h-[calc(100vh-240px)]">
          <thead><tr><Th>Time</Th><Th>Service</Th><Th>Root span</Th><Th>Route</Th><Th className="text-right">Status</Th><Th className="w-[18%]">Duration</Th><Th>User</Th><Th>Trace</Th></tr></thead>
          <tbody>
            {rows.map((r, i) => {
              const st = String(r[ix("status_code")]);
              const http = r[ix("http_status_code")] as number;
              const ms = Number(r[ix("duration_ms")]);
              return (
                <tr key={i} data-row className="hover:bg-panel-2/70">
                  <Td className="whitespace-nowrap text-muted">{fmtTime(String(r[ix("timestamp")]))}</Td>
                  <Td>{String(r[ix("service_name")])}</Td>
                  <Td><Link href={`/p/${pid}/traces/${r[ix("trace_id")]}`} className="hover:text-accent font-medium">{String(r[ix("name")])}</Link></Td>
                  <Td className="font-mono text-muted">{String(r[ix("http_route")] ?? "")}</Td>
                  <Td className="text-right"><Badge tone={st === "error" ? "err" : st === "ok" ? "ok" : "muted"}>{http ? http : st}</Badge></Td>
                  <Td>
                    <div className="flex items-center gap-2">
                      <span className="h-1.5 flex-1 rounded-full bg-panel-3"><span className={`block h-1.5 rounded-full ${ms >= 1000 ? "bg-warn shadow-[0_0_8px_var(--warn)]" : "bg-gradient-to-r from-accent-2 to-accent"}`} style={{ width: `${Math.max(3, (ms / maxMs) * 100)}%` }} /></span>
                      <span className="w-16 text-right font-mono tabular-nums">{fmtMs(ms)}</span>
                    </div>
                  </Td>
                  <Td className="font-mono">{r[ix("user_id")] ? <Link href={`/p/${pid}/users/${r[ix("user_id")]}`} className="text-cyan hover:underline">{String(r[ix("user_id")])}</Link> : <span className="text-faint">–</span>}</Td>
                  <Td><Link href={`/p/${pid}/traces/${r[ix("trace_id")]}`} className="font-mono text-cyan hover:underline">{String(r[ix("trace_id")]).slice(0, 12)}…</Link></Td>
                </tr>
              );
            })}
          </tbody>
        </Table>
      ))}
    </div>
  );
}
