"use client";

import { AssistantAction } from "@/components/assistant";
import { post as apiPostA } from "@/lib/api";

import { useParams, useRouter } from "next/navigation";
import Link from "next/link";
import { useState } from "react";
import { useProjectId, useProjectQuery, useProjectMutation, useRunQuery } from "@/lib/hooks";
import { put, del, post } from "@/lib/api";
import { Button, Card, Table, Th, Td, Empty, Badge, Drawer, ErrorBox, Stat } from "@/components/ui";
import { SeriesCharts } from "@/components/query/result-charts";
import { TriggerForm, draftOf, type TriggerDraft } from "../form";
import type { Incident, Trigger } from "@/lib/types";
import { ago, fmtNum, fmtTime, encodeQ } from "@/lib/format";

interface Ev { id: string; fired_at: string; state: string; value: number | null; message: string }
interface GroupState { group_key: string; severity: string; last_value: number | null; breaching_since: string | null; updated_at: string }

export default function TriggerPage() {
  const { triggerId } = useParams<{ triggerId: string }>();
  const pid = useProjectId();
  const router = useRouter();
  const t = useProjectQuery<{ trigger: Trigger; events: Ev[]; groups: GroupState[] }>(["trigger", triggerId], `/triggers/${triggerId}`, { refetchInterval: 15_000 });
  const [editing, setEditing] = useState(false);
  const update = useProjectMutation<TriggerDraft>((p, b) => put(`/api/projects/${p}/triggers/${triggerId}`, b), [["trigger", triggerId], ["triggers"]]);
  const remove = useProjectMutation<void>((p) => del(`/api/projects/${p}/triggers/${triggerId}`), [["triggers"]]);
  const trig = t.data?.trigger;
  const chartQ = trig ? { ...trig.query, time_range: { last_seconds: Math.max(trig.window_secs * 12, 3600) } } : null;
  const series = useRunQuery(chartQ);
  if (t.error) return <ErrorBox error={t.error} />;
  if (!trig) return <div className="text-muted">Loading…</div>;
  return (
    <div className="space-y-3">
      <div className="flex items-center gap-3">
        <Link href={`/p/${pid}/triggers`} className="text-muted hover:text-fg text-xs">← triggers</Link>
        <h1 className="text-base font-semibold">{trig.name}</h1>
        <Badge tone={trig.state === "triggered" ? (trig.severity === "warn" ? "warn" : "err") : trig.state === "error" ? "warn" : trig.state === "muted" ? "muted" : "ok"}>{trig.state === "triggered" ? trig.severity : trig.state}</Badge>
        {trig.mute_until && new Date(trig.mute_until) > new Date() && <Badge>muted until {fmtTime(trig.mute_until)}</Badge>}
        {trig.mode === "baseline" && <Badge tone="info">baseline ×{trig.baseline_factor}</Badge>}
        <div className="ml-auto flex gap-2">
          <Link href={`/p/${pid}/query?q=${encodeQ(chartQ)}`}><Button size="sm">Open in Query</Button></Link>
          <Button size="sm" onClick={() => setEditing(true)}>Edit</Button>
          <Button size="sm" variant="danger" onClick={async () => { if (confirm("Delete trigger?")) { await remove.mutateAsync(); router.replace(`/p/${pid}/triggers`); } }}>Delete</Button>
        </div>
      </div>
      {trig.description && <p className="text-muted text-sm">{trig.description}</p>}
      <div className="grid grid-cols-2 gap-3 md:grid-cols-4">
        <Stat label="Last value" value={fmtNum(trig.last_value)} sub={`threshold ${trig.op} ${trig.threshold}`} />
        <Stat label="Evaluated" value={ago(trig.last_evaluated_at)} sub={`every ${trig.frequency_secs}s over ${trig.window_secs}s`} />
        <Stat label="Last fired" value={ago(trig.last_triggered_at)} />
        <Stat label="Recipients" value={trig.recipients.length} sub={trig.recipients.map((r) => r.type).join(", ")} />
      </div>
      {series.data && series.data.groups.length > 0 && <SeriesCharts res={series.data} />}
      <IncidentsCard triggerId={trig.id} />
      <Card title="Investigate with Ask Galileo">
        <AssistantAction label="Investigate this trigger" run={() => apiPostA<{ markdown: string; recording?: { project_id: string; span_id: string; trace_id: string; model: string } }>(`/api/projects/${pid}/assistant/investigate`, { trigger_id: trig.id, last_seconds: 86400 })} />
      </Card>
      {trig.per_group && t.data!.groups.length > 0 && (
        <Card title="Per-group state">
          <Table><thead><tr><Th>Group</Th><Th>Severity</Th><Th className="text-right">Value</Th><Th>Breaching since</Th><Th></Th></tr></thead>
            <tbody>{t.data!.groups.map((g) => <tr key={g.group_key}><Td className="font-mono">{g.group_key || "∅"}</Td><Td><Badge tone={g.severity === "critical" ? "err" : g.severity === "warn" ? "warn" : "ok"}>{g.severity}</Badge></Td><Td className="text-right font-mono">{fmtNum(g.last_value)}</Td><Td className="text-muted">{g.breaching_since ? ago(g.breaching_since) : "–"}</Td><Td className="text-right"><MuteGroupButton triggerId={trig.id} groupKey={g.group_key} muted={(trig.mutes ?? []).some((m) => m.group_key === g.group_key && (!m.until || new Date(m.until) > new Date()))} /></Td></tr>)}</tbody></Table>
        </Card>
      )}
      <Card title="History">
        {t.data!.events.length === 0 ? <Empty>No state changes yet.</Empty> : (
          <Table><thead><tr><Th>When</Th><Th>State</Th><Th className="text-right">Value</Th><Th>Message</Th></tr></thead>
            <tbody>{t.data!.events.map((e) => <tr key={e.id}><Td className="text-muted whitespace-nowrap">{fmtTime(e.fired_at)}</Td><Td><Badge tone={e.state === "triggered" ? (e.message.includes("(warn;") ? "warn" : "err") : e.state === "error" ? "warn" : "ok"}>{e.state === "triggered" ? (e.message.includes("(warn;") ? "warn" : "critical") : e.state}</Badge></Td><Td className="text-right font-mono">{fmtNum(e.value)}</Td><Td className="text-muted">{e.message}</Td></tr>)}</tbody></Table>
        )}
      </Card>
      <Drawer open={editing} onClose={() => setEditing(false)} title="Edit trigger" width="w-[720px]">
        <TriggerForm initial={draftOf(trig)} error={update.error} onSubmit={async (d) => { await update.mutateAsync(d); setEditing(false); }} />
      </Drawer>
    </div>
  );
}

function IncidentsCard({ triggerId }: { triggerId: string }) {
  const q = useProjectQuery<{ incidents: Incident[] }>(["incidents", triggerId], `/triggers/${triggerId}/incidents`, { refetchInterval: 30_000 });
  const ack = useProjectMutation<string>((p, id) => post(`/api/projects/${p}/triggers/${triggerId}/incidents/${id}/ack`, { note: "" }), [["incidents", triggerId]]);
  if (!q.data || q.data.incidents.length === 0) return null;
  return (
    <Card title="Incidents">
      <Table><thead><tr><Th>Fired</Th><Th>Group</Th><Th>Severity</Th><Th className="text-right">Peak</Th><Th>Acknowledged</Th><Th>Resolved</Th><Th className="text-right">Repeats / esc.</Th><Th></Th></tr></thead>
        <tbody>{q.data.incidents.map((i) => <tr key={i.id}><Td className="whitespace-nowrap text-muted">{fmtTime(i.fired_at)}</Td><Td className="font-mono">{i.group_key || "–"}</Td><Td><Badge tone={i.severity === "critical" ? "err" : "warn"}>{i.severity}</Badge></Td><Td className="text-right font-mono">{fmtNum(i.peak_value)}</Td><Td className="text-muted">{i.acknowledged_at ? `${ago(i.acknowledged_at)} by ${i.acknowledged_by}` : "–"}</Td><Td className="text-muted">{i.resolved_at ? ago(i.resolved_at) : <Badge tone="err">open</Badge>}</Td><Td className="text-right">{i.notified} / {i.escalated}</Td><Td className="text-right">{!i.acknowledged_at && !i.resolved_at && <Button size="sm" onClick={() => ack.mutate(i.id)}>Ack</Button>}</Td></tr>)}</tbody></Table>
    </Card>
  );
}

function MuteGroupButton({ triggerId, groupKey, muted }: { triggerId: string; groupKey: string; muted: boolean }) {
  const m = useProjectMutation<{ group_key: string; hours: number; clear: boolean }>((p, b) => post(`/api/projects/${p}/triggers/${triggerId}/mute-group`, b), [["trigger", triggerId]]);
  return <Button size="sm" variant="ghost" onClick={() => m.mutate({ group_key: groupKey, hours: 24, clear: muted })}>{muted ? "unmute" : "mute 24h"}</Button>;
}
