"use client";

import Link from "next/link";
import { useState } from "react";
import clsx from "clsx";
import { useProjectId, useProjectQuery } from "@/lib/hooks";
import { Badge, Empty, ErrorBox, Input, Select } from "@/components/ui";
import type { Issue } from "@/lib/types";
import { ago, fmtNum } from "@/lib/format";

function Spark({ v }: { v: number[] }) {
  const max = Math.max(1, ...v);
  return <div className="flex h-5 items-end gap-px">{v.map((n, i) => <div key={i} className={clsx("w-1.5 rounded-sm", n ? "bg-err/70" : "bg-panel-2")} style={{ height: `${Math.max(8, (n / max) * 100)}%` }} />)}</div>;
}

export default function IssuesPage() {
  const pid = useProjectId();
  const [status, setStatus] = useState("open");
  const [sort, setSort] = useState("last_seen");
  const [q, setQ] = useState("");
  const [last, setLast] = useState(86400);
  const list = useProjectQuery<{ issues: Issue[]; counts: Record<string, number> }>(["issues", status, sort, q, last], `/issues?status=${status}&sort=${sort}&q=${encodeURIComponent(q)}&last_seconds=${last}`, { refetchInterval: 30_000 });
  const counts = list.data?.counts ?? {};
  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <div className="flex gap-1 border-b">
          {(["open", "resolved", "ignored", "all"] as const).map((t) => (
            <button key={t} onClick={() => setStatus(t)} className={clsx("px-3 py-1.5 text-sm capitalize border-b-2 -mb-px", status === t ? "border-accent text-fg" : "border-transparent text-muted hover:text-fg")}>
              {t}{t !== "all" && counts[t] ? <span className="ml-1 text-[10px] text-muted">{counts[t]}</span> : null}
            </button>
          ))}
        </div>
        <Input data-page-filter className="w-64" placeholder="search title, culprit, route" value={q} onChange={(e) => setQ(e.target.value)} />
        <Select value={sort} onChange={(e) => setSort(e.target.value)}><option value="last_seen">Last seen</option><option value="count">Most events</option><option value="users">Most users</option><option value="first_seen">Newest</option></Select>
        <Select value={last} onChange={(e) => setLast(Number(e.target.value))}>{[3600, 86400, 7 * 86400].map((s) => <option key={s} value={s}>{s >= 86400 ? `${s / 86400}d` : `${s / 3600}h`} window</option>)}</Select>
      </div>
      <ErrorBox error={list.error} />
      {list.data && list.data.issues.length === 0 && <Empty>No {status === "all" ? "" : status} issues. Exceptions recorded on request spans are grouped here automatically.</Empty>}
      <div className="divide-y rounded-md border">
        {list.data?.issues.map((i) => (
          <Link key={i.id} data-row href={`/p/${pid}/issues/${i.id}`} className="flex flex-wrap items-center gap-3 px-3 py-2 hover:bg-panel-2">
            <div className="min-w-0 flex-1 basis-64">
              <div className="flex items-center gap-2">
                <span className={clsx("truncate font-medium", i.status === "open" ? "text-fg" : "text-muted")}>{i.title}</span>
                {i.status !== "open" && <Badge tone={i.status === "resolved" ? "ok" : "muted"}>{i.status}</Badge>}
                {i.last_version && <Badge>{i.last_version}</Badge>}
              </div>
              <div className="mt-0.5 flex gap-2 text-[11px] text-muted font-mono truncate">
                <span className="text-accent/80">{i.culprit || i.exception_type}</span>
                {i.route && <span>{i.route}</span>}
                <span>{i.service_name}</span>
              </div>
            </div>
            <Spark v={i.sparkline ?? []} />
            <div className="w-16 text-right"><div className="tabular-nums font-semibold">{fmtNum(i.window_count ?? 0)}</div><div className="text-[10px] text-muted">in window</div></div>
            <div className="w-16 text-right"><div className="tabular-nums">{fmtNum(i.count)}</div><div className="text-[10px] text-muted">total</div></div>
            <div className="w-12 text-right"><div className="tabular-nums">{fmtNum(i.users)}</div><div className="text-[10px] text-muted">users</div></div>
            <div className="w-20 text-right text-[11px] text-muted">{ago(i.last_seen)}<div className="text-[10px]">first {ago(i.first_seen)}</div></div>
          </Link>
        ))}
      </div>
    </div>
  );
}
