"use client";

import { useParams } from "next/navigation";
import Link from "next/link";
import clsx from "clsx";
import { useProjectId, useProjectQuery } from "@/lib/hooks";
import type { SessionRow } from "@/lib/types";
import { Badge, Empty, ErrorBox, Stat, Card, PageHeader } from "@/components/ui";
import { ago, fmtMs, fmtNum, fmtTime, fmtUsd } from "@/lib/format";
import { useLastSeconds } from "@/lib/time-range";

interface Ev { timestamp: string; kind: "request" | "log"; name: string; route: string; status: number; status_code: string; duration_ms: number; trace_id: string; tenant_id: string; service_name: string; body: string; severity: string; model: string; cost_usd: number }
interface Summary { requests: number; errors: number; llm_calls: number; llm_cost_usd: number; tenants: number; main_tenant: string; first_seen: string; last_seen: string; routes: number }

const SEV: Record<string, "err" | "warn" | "ok" | "muted"> = { fatal: "err", error: "err", warn: "warn", info: "ok" };

export default function UserPage() {
  const { userId } = useParams<{ userId: string }>();
  const pid = useProjectId();
  const [last] = useLastSeconds();
  const q = useProjectQuery<{ summary: Summary | null; events: Ev[] }>(["user-timeline", userId, last], `/users/${encodeURIComponent(userId)}/timeline?last_seconds=${last}`);
  const s = q.data?.summary;
  const sessions = useProjectQuery<{ sessions: SessionRow[] }>(["sessions", last, userId], `/sessions?last_seconds=${last}&user_id=${encodeURIComponent(userId)}&limit=50`);
  return (
    <div className="mx-auto max-w-[1400px] space-y-4">
      <PageHeader title={<>User <span className="font-mono">{userId}</span></>} sub={s?.main_tenant ? <>tenant {s.main_tenant}{s.tenants > 1 ? ` +${s.tenants - 1}` : ""}</> : "Everything this user did in the window"} />
      <ErrorBox error={q.error} />
      {sessions.data && sessions.data.sessions.length > 0 && (
        <Card title={`Browser sessions (${sessions.data.sessions.length})`}>
          <ul className="space-y-1 text-xs">{sessions.data.sessions.map((r) => (
            <li key={r.session_id} className="flex items-center gap-2 flex-wrap">
              <Link href={`/p/${pid}/browser/sessions/${r.session_id}`} className="font-mono text-info hover:underline">{r.session_id.slice(0, 8)}…</Link>
              <span className="text-muted">{ago(r.last_seen)}</span><span>{r.browser}</span>
              <span className="text-muted">{fmtNum(r.pages, 0)} pages · {fmtNum(r.fetches, 0)} fetches</span>
              {r.errors ? <Badge tone="err">{fmtNum(r.errors, 0)} JS errors</Badge> : null}
              <span className="font-mono text-muted">{r.first_path}{r.last_path !== r.first_path ? ` → ${r.last_path}` : ""}</span>
            </li>))}</ul>
        </Card>
      )}
      {s && (
        <div className="grid grid-cols-2 gap-3 md:grid-cols-5">
          <Stat label="Requests" value={fmtNum(s.requests)} sub={`${s.routes} routes`} />
          <Stat label="Errors" value={s.errors} tone={s.errors ? "err" : undefined} />
          <Stat label="LLM calls" value={s.llm_calls} sub={fmtUsd(s.llm_cost_usd)} />
          <Stat label="First seen" value={ago(s.first_seen)} sub={fmtTime(s.first_seen)} />
          <Stat label="Last seen" value={ago(s.last_seen)} sub={fmtTime(s.last_seen)} />
        </div>
      )}
      {q.data && q.data.events.length === 0 && <Empty>Nothing recorded for this user in the window.</Empty>}
      {!!q.data?.events.length && (
        <div className="rounded-md border overflow-auto scroll-thin max-h-[calc(100vh-260px)] text-[12px]">
          {q.data.events.map((e, i) => (
            <div key={i} className="flex items-start gap-2 border-b border-border/50 px-2 py-1 hover:bg-panel-2">
              <span className="w-32 shrink-0 font-mono text-muted">{fmtTime(e.timestamp)}</span>
              {e.kind === "request" ? (
                e.model ? (
                  <>
                    <Badge tone="accent" className="w-16 justify-center">LLM</Badge>
                    <span className="font-mono">{e.model}</span>
                    <span className="text-muted">{fmtMs(e.duration_ms)} · {fmtUsd(e.cost_usd)}</span>
                    {e.status_code === "error" && <Badge tone="err">error</Badge>}
                  </>
                ) : (
                  <>
                    <Badge tone={e.status >= 500 || e.status_code === "error" ? "err" : e.status >= 400 ? "warn" : "ok"} className="w-16 justify-center">{e.status || e.status_code}</Badge>
                    <span className={clsx("font-mono", e.status >= 500 && "text-err")}>{e.name}</span>
                    <span className="text-muted">{fmtMs(e.duration_ms)}</span>
                  </>
                )
              ) : (
                <>
                  <Badge tone={SEV[e.severity] ?? "muted"} className="w-16 justify-center">{e.severity}</Badge>
                  <span className="font-mono whitespace-pre-wrap break-all">{e.body}</span>
                </>
              )}
              {e.trace_id && <Link href={`/p/${pid}/traces/${e.trace_id}`} className="ml-auto shrink-0 font-mono text-info hover:underline">trace</Link>}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
