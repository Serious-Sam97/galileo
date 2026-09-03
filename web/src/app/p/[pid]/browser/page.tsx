"use client";

import { useState } from "react";
import Link from "next/link";
import { useProjectId, useProjectQuery } from "@/lib/hooks";
import { Card, Stat, Table, Th, Td, Empty, ErrorBox, Badge, Input, Select } from "@/components/ui";
import { VitalPill, VITAL_LABELS, vitalTone, fmtVital } from "@/components/vitals";
import { fmtNum, fmtTime, ago } from "@/lib/format";
import type { SessionRow, VitalsRes } from "@/lib/types";

const ORDER = ["lcp", "inp", "cls", "fcp", "ttfb", "fid"];

export default function BrowserPage() {
  const pid = useProjectId();
  const [last, setLast] = useState(86400);
  const [user, setUser] = useState("");
  const v = useProjectQuery<VitalsRes>(["rum-vitals", last], `/rum/vitals?last_seconds=${last}`);
  const s = useProjectQuery<{ sessions: SessionRow[] }>(["sessions", last, user], `/sessions?last_seconds=${last}&user_id=${encodeURIComponent(user)}&limit=200`);
  const overall = Object.fromEntries((v.data?.overall ?? []).map((o) => [o.name, o]));
  const noData = v.data && v.data.counts.sessions === 0 && (s.data?.sessions.length ?? 0) === 0;
  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <h1 className="text-base font-semibold mr-2">Browser</h1>
        <Select value={last} onChange={(e) => setLast(Number(e.target.value))}>{[3600, 86400, 7 * 86400, 30 * 86400].map((x) => <option key={x} value={x}>Last {x >= 86400 ? x / 86400 + "d" : x / 3600 + "h"}</option>)}</Select>
        <Input className="w-56" placeholder="filter sessions by user id" value={user} onChange={(e) => setUser(e.target.value)} />
      </div>
      <ErrorBox error={v.error ?? s.error} />
      {noData && <Empty>No browser data yet. Settings → Connect → Browser shows the one-line script tag; see docs/rum.md.</Empty>}
      {v.data && (
        <div className="grid grid-cols-2 gap-3 md:grid-cols-6">
          <Stat label="Sessions" value={fmtNum(v.data.counts.sessions, 0)} sub={`${fmtNum(v.data.counts.users, 0)} users`} />
          <Stat label="Page views" value={fmtNum(v.data.counts.pages, 0)} />
          <Stat label="Fetches" value={fmtNum(v.data.counts.fetches, 0)} sub={`${fmtNum(v.data.counts.failed_fetches, 0)} failed`} tone={v.data.counts.failed_fetches ? "warn" : undefined} />
          <Stat label="JS errors" value={fmtNum(v.data.counts.errors, 0)} tone={v.data.counts.errors ? "err" : undefined} />
          {["lcp", "inp"].map((n) => <Stat key={n} label={`${VITAL_LABELS[n]} p75`} value={fmtVital(n, overall[n]?.p75)} sub={overall[n] ? `${fmtNum(overall[n].n, 0)} samples` : "no samples"} tone={vitalTone(n, overall[n]?.p75)} />)}
        </div>
      )}
      {v.data && v.data.overall.length > 0 && (
        <Card title="Web Vitals (p75, whole window)">
          <div className="flex flex-wrap gap-2">{ORDER.filter((n) => overall[n]).map((n) => <VitalPill key={n} name={n} value={overall[n].p75} />)}</div>
          <p className="mt-2 text-[11px] text-muted">Green = good, amber = needs improvement, red = poor (web.dev thresholds). LCP: largest paint · INP: interaction latency · CLS: layout shift · FCP: first paint · TTFB: server response.</p>
        </Card>
      )}
      {v.data && v.data.by_page.length > 0 && (
        <Card title="Vitals by page">
          <Table><thead><tr><Th>Page</Th><Th className="text-right">Views</Th>{ORDER.map((n) => <Th key={n} className="text-right">{VITAL_LABELS[n]} p75</Th>)}</tr></thead>
            <tbody>{[...v.data.by_page].sort((a, b) => b.n - a.n).map((r) => <tr key={r.path}><Td className="font-mono">{r.path}</Td><Td className="text-right">{fmtNum(r.n, 0)}</Td>{ORDER.map((n) => <Td key={n} className="text-right"><VitalPill name={n} value={r.vitals[n]} /></Td>)}</tr>)}</tbody></Table>
        </Card>
      )}
      <Card title="Sessions">
        {!s.data ? null : s.data.sessions.length === 0 ? <Empty>No sessions in this window.</Empty> : (
          <Table className="max-h-[60vh]"><thead><tr><Th>Last seen</Th><Th>User</Th><Th>Browser</Th><Th>Service</Th><Th className="text-right">Pages</Th><Th className="text-right">Fetches</Th><Th className="text-right">Errors</Th><Th className="text-right">LCP p75</Th><Th>Path</Th></tr></thead>
            <tbody>{s.data.sessions.map((r) => (
              <tr key={r.session_id} className="hover:bg-panel-2">
                <Td className="whitespace-nowrap"><Link href={`/p/${pid}/browser/sessions/${r.session_id}`} className="text-info hover:underline" title={fmtTime(r.last_seen)}>{ago(r.last_seen)}</Link></Td>
                <Td className="font-mono">{r.user_id ? <Link href={`/p/${pid}/users/${encodeURIComponent(r.user_id)}`} className="hover:underline">{r.user_id}</Link> : <span className="text-muted">anonymous</span>}</Td>
                <Td>{r.browser}</Td><Td className="text-muted">{r.service_name}</Td>
                <Td className="text-right">{fmtNum(r.pages, 0)}</Td>
                <Td className="text-right">{fmtNum(r.fetches, 0)}{r.failed_fetches ? <span className="text-warn"> ({fmtNum(r.failed_fetches, 0)} failed)</span> : null}</Td>
                <Td className="text-right">{r.errors ? <Badge tone="err">{fmtNum(r.errors, 0)}</Badge> : <span className="text-muted">0</span>}</Td>
                <Td className="text-right"><VitalPill name="lcp" value={r.p75_lcp || undefined} /></Td>
                <Td className="font-mono text-muted text-[11px]">{r.first_path}{r.last_path !== r.first_path ? ` → ${r.last_path}` : ""}</Td>
              </tr>))}</tbody></Table>
        )}
      </Card>
    </div>
  );
}
