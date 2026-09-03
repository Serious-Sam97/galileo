"use client";

import Link from "next/link";
import { useParams, useRouter } from "next/navigation";
import { useEffect, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { get } from "@/lib/api";
import { useMe } from "@/lib/hooks";
import { Badge, Empty, ErrorBox, Select } from "@/components/ui";
import type { OrgProject } from "@/lib/types";
import { ago, fmtMs, fmtNum, fmtUsd } from "@/lib/format";
import { Telescope } from "lucide-react";

export default function OrgOverviewPage() {
  const { orgId } = useParams<{ orgId: string }>();
  const me = useMe();
  const router = useRouter();
  useEffect(() => { if (me.isError) router.replace("/login"); }, [me.isError, router]);
  const [last, setLast] = useState(3600);
  const ov = useQuery({ queryKey: ["org-overview", orgId, last], queryFn: () => get<{ projects: OrgProject[] }>(`/api/orgs/${orgId}/overview?last_seconds=${last}`), refetchInterval: 30_000 });
  const org = me.data?.orgs.find((o) => o.id === orgId);
  const totals = (ov.data?.projects ?? []).reduce((a, p) => ({ req: a.req + (p.requests ?? 0), err: a.err + (p.errors ?? 0), cost: a.cost + (p.llm_cost_usd ?? 0), issues: a.issues + p.open_issues }), { req: 0, err: 0, cost: 0, issues: 0 });
  return (
    <div className="min-h-screen p-6 space-y-4">
      <div className="flex items-center gap-3">
        <Telescope className="text-accent" size={22} />
        <h1 className="text-lg font-semibold">{org?.name ?? "Organization"}</h1>
        <span className="text-muted text-sm">{ov.data?.projects.length ?? 0} projects · {fmtNum(totals.req)} requests · {totals.err} errors · {fmtUsd(totals.cost)} LLM · {totals.issues} open issues</span>
        <div className="ml-auto flex items-center gap-3">
          <Select value={last} onChange={(e) => setLast(Number(e.target.value))}>{[900, 3600, 86400, 7 * 86400].map((s) => <option key={s} value={s}>Last {s >= 86400 ? s / 86400 + "d" : s >= 3600 ? s / 3600 + "h" : s / 60 + "m"}</option>)}</Select>
          {me.data?.orgs.length ? <Select value={orgId} onChange={(e) => router.push(`/org/${e.target.value}`)}>{me.data.orgs.map((o) => <option key={o.id} value={o.id}>{o.name}</option>)}</Select> : null}
        </div>
      </div>
      <ErrorBox error={ov.error} />
      {ov.data && ov.data.projects.length === 0 && <Empty>No projects yet. Create one from any project&apos;s Settings → Project.</Empty>}
      <div className="grid gap-3" style={{ gridTemplateColumns: "repeat(auto-fill, minmax(300px, 1fr))" }}>
        {ov.data?.projects.map((p) => {
          const errPct = p.requests ? (p.errors ?? 0) / p.requests : 0;
          const quiet = !p.requests;
          return (
            <Link key={p.id} href={`/p/${p.id}/overview`} className="rounded-lg border bg-panel p-4 hover:border-accent/60">
              <div className="flex items-center justify-between">
                <div className="font-semibold">{p.name}</div>
                <div className="flex gap-1">{p.open_issues > 0 && <Badge tone="err">{p.open_issues} issues</Badge>}{quiet ? <Badge>quiet</Badge> : errPct > 0.05 ? <Badge tone="err">{(errPct * 100).toFixed(1)}% errors</Badge> : <Badge tone="ok">healthy</Badge>}</div>
              </div>
              <div className="mt-3 grid grid-cols-3 gap-2 text-center">
                <div><div className="text-lg font-semibold tabular-nums">{fmtNum(p.requests ?? 0)}</div><div className="text-[10px] uppercase text-muted">requests</div></div>
                <div><div className="text-lg font-semibold tabular-nums">{fmtMs(p.p95_ms ?? 0)}</div><div className="text-[10px] uppercase text-muted">p95</div></div>
                <div><div className="text-lg font-semibold tabular-nums">{fmtUsd(p.llm_cost_usd ?? 0)}</div><div className="text-[10px] uppercase text-muted">{p.llm_calls ?? 0} LLM calls</div></div>
              </div>
              <div className="mt-3 flex justify-between text-[11px] text-muted"><span>{p.services ?? 0} services · {p.users ?? 0} users</span><span>{p.last_seen ? `seen ${ago(p.last_seen)}` : "no data in window"}</span></div>
            </Link>
          );
        })}
      </div>
    </div>
  );
}
