"use client";

import { AssistantAction } from "@/components/assistant";
import { post as apiPostA } from "@/lib/api";

import { useParams } from "next/navigation";
import Link from "next/link";
import { useMemo, useState } from "react";
import { useProjectId, useProjectQuery, useProjectMutation } from "@/lib/hooks";
import { post } from "@/lib/api";
import { Badge, Button, Card, Drawer, Empty, ErrorBox, Input, Label, Stat, Table, Th, Td, Textarea } from "@/components/ui";
import { Chart, axisStyle, type EChartsOption } from "@/components/charts/chart";
import type { Issue } from "@/lib/types";
import { ago, fmtMs, fmtNum, fmtTime } from "@/lib/format";

interface Detail {
  issue: Issue; events: { id: string; kind: string; at: string; message: string }[]; start: string; end: string; granularity: number;
  series: { ts: number; n: number }[]; routes: { key: string; n: number }[]; users: { key: string; n: number }[]; tenants: { key: string; n: number }[]; versions: { key: string; n: number }[];
  samples: { timestamp: string; trace_id: string; http_route: string; user_id: string; tenant_id: string; duration_ms: number; exception_message: string; service_version: string }[];
  latest: { stack: string; exception_message: string; timestamp: string; trace_id: string } | null;
}

function Breakdown({ title, rows, link }: { title: string; rows: { key: string; n: number }[]; link?: (k: string) => string }) {
  const total = rows.reduce((a, r) => a + r.n, 0) || 1;
  return (
    <Card title={title}>
      {rows.length === 0 ? <div className="text-muted text-xs">none</div> : rows.map((r) => (
        <div key={r.key} className="mb-1.5">
          <div className="flex justify-between text-[11px]"><span className="font-mono truncate">{link ? <Link className="text-info hover:underline" href={link(r.key)}>{r.key}</Link> : r.key}</span><span className="text-muted tabular-nums">{fmtNum(r.n)} · {Math.round((r.n / total) * 100)}%</span></div>
          <div className="h-1 rounded bg-panel-2"><div className="h-1 rounded bg-err/70" style={{ width: `${(r.n / total) * 100}%` }} /></div>
        </div>
      ))}
    </Card>
  );
}

export default function IssuePage() {
  const { issueId } = useParams<{ issueId: string }>();
  const pid = useProjectId();
  const [last, setLast] = useState(86400);
  const d = useProjectQuery<Detail>(["issue", issueId, last], `/issues/${issueId}?last_seconds=${last}`, { refetchInterval: 30_000 });
  const [action, setAction] = useState<"resolve" | "ignore" | "reopen" | null>(null);
  const [note, setNote] = useState("");
  const [version, setVersion] = useState("");
  const act = useProjectMutation<{ a: string; note: string; version: string }>((p, b) => post(`/api/projects/${p}/issues/${issueId}/${b.a}`, { note: b.note, version: b.version }), [["issue", issueId], ["issues"], ["issue-counts"]]);
  const option = useMemo<EChartsOption>(() => {
    if (!d.data) return {};
    return {
      tooltip: { trigger: "axis", backgroundColor: "#161c29", borderColor: "#232a3a", textStyle: { color: "#e6e9ef", fontSize: 11 } },
      grid: { left: 40, right: 12, top: 12, bottom: 24 },
      xAxis: { type: "time", ...axisStyle },
      yAxis: { type: "value", ...axisStyle, minInterval: 1 },
      series: [{ type: "bar", name: "occurrences", data: d.data.series.map((p) => [Number(p.ts) * 1000, Number(p.n)]), itemStyle: { color: "#ff5c6c" }, barMaxWidth: 12 }],
    };
  }, [d.data]);
  if (d.error) return <ErrorBox error={d.error} />;
  if (!d.data) return <div className="text-muted">Loading…</div>;
  const i = d.data.issue;
  return (
    <div className="space-y-3">
      <div className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2"><Link href={`/p/${pid}/issues`} className="text-muted hover:text-fg text-xs">← issues</Link>
            <Badge tone={i.status === "open" ? "err" : i.status === "resolved" ? "ok" : "muted"}>{i.status}</Badge>{i.last_version && <Badge>last seen on {i.last_version}</Badge>}</div>
          <h1 className="mt-1 text-base font-semibold break-words">{i.title}</h1>
          <div className="mt-0.5 flex flex-wrap gap-3 text-[12px] font-mono text-muted"><span className="text-accent">{i.culprit || i.exception_type}</span><span>{i.route}</span><span>{i.service_name}</span></div>
        </div>
        <div className="flex gap-2">
          {i.status !== "resolved" && <Button size="sm" variant="primary" onClick={() => setAction("resolve")}>Resolve</Button>}
          {i.status !== "ignored" && <Button size="sm" onClick={() => setAction("ignore")}>Ignore</Button>}
          {i.status !== "open" && <Button size="sm" onClick={() => setAction("reopen")}>Reopen</Button>}
        </div>
      </div>
      <div className="grid grid-cols-2 gap-3 md:grid-cols-5">
        <Stat label="Events (total)" value={fmtNum(i.count)} />
        <Stat label="Users" value={fmtNum(i.users)} />
        <Stat label="First seen" value={ago(i.first_seen)} sub={fmtTime(i.first_seen)} />
        <Stat label="Last seen" value={ago(i.last_seen)} sub={fmtTime(i.last_seen)} />
        <Stat label={i.status === "resolved" ? "Resolved" : "Window"} value={i.status === "resolved" && i.resolved_at ? ago(i.resolved_at) : `${last >= 86400 ? last / 86400 + "d" : last / 3600 + "h"}`} sub={i.resolved_version ? `in ${i.resolved_version}` : undefined} />
      </div>
      <Card title="Occurrences" actions={<select className="rounded-md border bg-bg px-2 py-0.5 text-xs" value={last} onChange={(e) => setLast(Number(e.target.value))}>{[3600, 86400, 7 * 86400].map((s) => <option key={s} value={s}>{s >= 86400 ? `${s / 86400}d` : `${s / 3600}h`}</option>)}</select>}>
        <Chart option={option} height={160} />
      </Card>
      <div className="grid gap-3 md:grid-cols-2">
        <Card title={`Latest stack trace${d.data.latest ? ` · ${fmtTime(d.data.latest.timestamp)}` : ""}`} actions={d.data.latest ? <Link href={`/p/${pid}/traces/${d.data.latest.trace_id}`} className="text-xs text-info hover:underline">open trace</Link> : null}>
          {d.data.latest ? <pre className="max-h-96 overflow-auto scroll-thin whitespace-pre-wrap rounded border bg-bg p-2 font-mono text-[11px]">{d.data.latest.stack || d.data.latest.exception_message || "(no stack trace recorded)"}</pre> : <Empty>No occurrence in the last 30 days.</Empty>}
        </Card>
        <div className="grid gap-3 sm:grid-cols-2">
          <Breakdown title="Routes" rows={d.data.routes} />
          <Breakdown title="Users" rows={d.data.users} link={(k) => `/p/${pid}/users/${k}`} />
          <Breakdown title="Clinics / tenants" rows={d.data.tenants} />
          <Breakdown title="Versions" rows={d.data.versions} />
        </div>
      </div>
      <Card title="Recent occurrences">
        <Table><thead><tr><Th>When</Th><Th>Route</Th><Th>User</Th><Th>Tenant</Th><Th>Version</Th><Th className="text-right">Duration</Th><Th>Message</Th><Th></Th></tr></thead>
          <tbody>{d.data.samples.map((s, k) => <tr key={k}><Td className="whitespace-nowrap text-muted">{fmtTime(s.timestamp)}</Td><Td className="font-mono">{s.http_route}</Td><Td className="font-mono">{s.user_id ? <Link className="text-info hover:underline" href={`/p/${pid}/users/${s.user_id}`}>{s.user_id}</Link> : "–"}</Td><Td>{s.tenant_id}</Td><Td>{s.service_version}</Td><Td className="text-right font-mono">{fmtMs(Number(s.duration_ms))}</Td><Td className="max-w-[360px] truncate text-muted" title={s.exception_message}>{s.exception_message}</Td><Td><Link href={`/p/${pid}/traces/${s.trace_id}`} className="text-info hover:underline">trace</Link></Td></tr>)}</tbody></Table>
      </Card>
      <Card title="Investigate with Ask Galileo">
        <AssistantAction label="Investigate this issue" run={() => apiPostA<{ markdown: string; recording?: { project_id: string; span_id: string; trace_id: string; model: string } }>(`/api/projects/${pid}/assistant/investigate`, { issue_id: i.id, last_seconds: 7 * 86400 })} onDone={() => d.refetch()} />
      </Card>
      <Card title="History">
        {d.data.events.map((e) => <div key={e.id} className="flex gap-3 border-b border-border/50 py-1 text-[12px]"><span className="w-32 shrink-0 text-muted">{fmtTime(e.at)}</span><Badge tone={e.kind === "regressed" || e.kind === "new" ? "err" : e.kind === "resolved" ? "ok" : "muted"}>{e.kind}</Badge><span className="text-muted">{e.message}</span></div>)}
        {i.notes && <div className="mt-2 text-[12px]"><span className="text-muted">notes: </span>{i.notes}</div>}
      </Card>
      <Drawer open={!!action} onClose={() => setAction(null)} title={action ? `${action[0].toUpperCase()}${action.slice(1)} issue` : ""}>
        <div className="space-y-3">
          {action === "resolve" && <div><Label>Fixed in version (optional)</Label><Input value={version} onChange={(e) => setVersion(e.target.value)} placeholder="1.4.3" /><p className="mt-1 text-xs text-muted">If it happens again the issue reopens as <b>regressed</b> and notifies.</p></div>}
          <div><Label>Note</Label><Textarea rows={3} value={note} onChange={(e) => setNote(e.target.value)} /></div>
          <ErrorBox error={act.error} />
          <Button variant="primary" onClick={async () => { await act.mutateAsync({ a: action!, note, version }); setAction(null); setNote(""); }}>Confirm</Button>
        </div>
      </Drawer>
    </div>
  );
}
