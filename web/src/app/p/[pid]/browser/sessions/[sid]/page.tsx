"use client";

import { use } from "react";
import Link from "next/link";
import clsx from "clsx";
import { useProjectId, useProjectQuery } from "@/lib/hooks";
import { Card, Stat, Empty, ErrorBox, Badge } from "@/components/ui";
import { VitalPill } from "@/components/vitals";
import { fmtMs, fmtTime, fmtNum } from "@/lib/format";
import type { SessionDetail, SessionEvent } from "@/lib/types";

const VITAL_KEYS = ["lcp", "inp", "cls", "fcp", "ttfb"];

export default function SessionPage({ params }: { params: Promise<{ sid: string }> }) {
  const { sid } = use(params);
  const pid = useProjectId();
  const q = useProjectQuery<SessionDetail>(["session", sid], `/sessions/${sid}`);
  const d = q.data;
  // group: each page view (pageload/navigation) owns the events that follow it
  const groups: { page: SessionEvent | null; items: SessionEvent[] }[] = [];
  for (const e of d?.events ?? []) {
    const t = e.attrs["rum.type"];
    if (t === "pageload" || t === "navigation") groups.push({ page: e, items: [] });
    else { if (!groups.length) groups.push({ page: null, items: [] }); groups[groups.length - 1].items.push(e); }
  }
  const errors = d?.events.filter((e) => e.attrs["rum.type"] === "error").length ?? 0;
  const fetches = d?.events.filter((e) => e.attrs["rum.type"] === "fetch") ?? [];
  const first = d?.events[0], last = d?.events[d.events.length - 1];
  const spanMs = first && last ? new Date(last.timestamp).getTime() - new Date(first.timestamp).getTime() : 0;
  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2 flex-wrap">
        <Link href={`/p/${pid}/browser`} className="text-muted hover:text-fg text-sm">← Browser</Link>
        <h1 className="text-base font-semibold font-mono">session {sid.slice(0, 8)}…</h1>
        {d?.user_id && <Link href={`/p/${pid}/users/${encodeURIComponent(d.user_id)}`} className="text-info hover:underline text-sm">user {d.user_id}</Link>}
        {first && <span className="text-muted text-xs">{fmtTime(first.timestamp)} · {first.attrs["browser.name"]} · {first.service_name}</span>}
      </div>
      <ErrorBox error={q.error} />
      {d && (
        <div className="grid grid-cols-2 gap-3 md:grid-cols-5">
          <Stat label="Duration" value={spanMs >= 60000 ? `${Math.round(spanMs / 60000)} min` : fmtMs(spanMs)} />
          <Stat label="Page views" value={groups.filter((g) => g.page).length} />
          <Stat label="Fetches" value={fetches.length} sub={`${fetches.filter((f) => f.status_code === "error").length} failed`} />
          <Stat label="JS errors" value={errors} tone={errors ? "err" : undefined} />
          <Stat label="Backend time" value={fmtMs(fetches.reduce((a, f) => a + (f.backend?.duration_ms ?? 0), 0))} sub={`${fmtNum(fetches.reduce((a, f) => a + (f.backend?.db_calls ?? 0), 0), 0)} DB calls`} />
        </div>
      )}
      {d && d.events.length === 0 && <Empty>No events for this session.</Empty>}
      {groups.map((g, gi) => (
        <Card key={gi} title={g.page ? <span className="flex items-center gap-2 flex-wrap"><Badge tone="accent">{g.page.attrs["rum.type"]}</Badge><span className="font-mono">{g.page.attrs["url.path"]}</span><span className="text-muted text-xs font-normal">{fmtTime(g.page.timestamp)}</span>{g.page.attrs["rum.type"] === "pageload" && <span className="text-muted text-xs font-normal">load {fmtMs(g.page.duration_ms)} · TTFB {g.page.attrs["navigation.ttfb_ms"] ?? "?"}ms · DCL {g.page.attrs["navigation.dom_content_loaded_ms"] ?? "?"}ms</span>}<Link href={`/p/${pid}/traces/${g.page.trace_id}`} className="text-info text-xs font-normal hover:underline ml-auto">trace →</Link></span> : "before first page view"}>
          {g.page && VITAL_KEYS.some((k) => g.page!.attrs[`web_vital.${k}`]) && <div className="mb-2 flex gap-1.5 flex-wrap">{VITAL_KEYS.filter((k) => g.page!.attrs[`web_vital.${k}`]).map((k) => <VitalPill key={k} name={k} value={Number(g.page!.attrs[`web_vital.${k}`])} />)}</div>}
          {g.items.length === 0 ? <p className="text-xs text-muted">No fetches or errors on this page view.</p> : (
            <ol className="space-y-1">
              {g.items.map((e) => <EventRow key={e.span_id} e={e} pid={pid} base={g.page ? new Date(g.page.timestamp).getTime() : new Date(e.timestamp).getTime()} />)}
            </ol>
          )}
        </Card>
      ))}
    </div>
  );
}

function EventRow({ e, pid, base }: { e: SessionEvent; pid: string; base: number }) {
  const t = e.attrs["rum.type"];
  const rel = new Date(e.timestamp).getTime() - base;
  const relText = rel >= 60000 ? `+${(rel / 60000).toFixed(1)}min` : rel >= 1000 ? `+${(rel / 1000).toFixed(1)}s` : `+${Math.max(0, Math.round(rel))}ms`;
  if (t === "error") {
    return (
      <li className="rounded border border-err/40 bg-err/5 p-2 text-xs">
        <div className="flex items-center gap-2"><span className="text-muted font-mono w-16 shrink-0">{relText}</span><Badge tone="err">JS error</Badge><span className="font-mono">{e.attrs["exception.type"]}</span><span className="truncate">{e.attrs["exception.message"]}</span>
          <Link href={`/p/${pid}/issues?q=${encodeURIComponent(e.attrs["exception.type"] ?? "")}`} className="ml-auto text-info hover:underline shrink-0">issues →</Link></div>
        {e.attrs["exception.stacktrace"] && <pre className="mt-1 max-h-24 overflow-auto scroll-thin whitespace-pre-wrap font-mono text-[10px] text-muted">{e.attrs["exception.stacktrace"]}</pre>}
      </li>
    );
  }
  if (t === "fetch") {
    const st = Number(e.attrs["http.response.status_code"] ?? 0);
    const b = e.backend;
    return (
      <li className="text-xs">
        <div className="flex items-center gap-2"><span className="text-muted font-mono w-16 shrink-0">{relText}</span>
          <Badge tone={e.status_code === "error" ? "err" : "muted"}>{st || "net"}</Badge><span className="font-mono">{e.attrs["http.request.method"]} {e.attrs["http.url.path"]}</span><span className="text-muted">{fmtMs(e.duration_ms)}</span>
          <Link href={`/p/${pid}/traces/${e.trace_id}`} className="ml-auto text-info hover:underline shrink-0">trace →</Link></div>
        {b ? (
          <div className={clsx("ml-16 mt-0.5 flex items-center gap-2 rounded border-l-2 pl-2 text-[11px]", b.status_code === "error" ? "border-err" : "border-accent/50")}>
            <span className="text-muted">↳ {b.service_name}</span><span className="font-mono">{b.http_route || b.name}</span><span className="text-muted">{fmtMs(b.duration_ms)}</span>
            {b.db_calls > 0 && <span className="text-muted">{fmtNum(b.db_calls, 0)} DB calls</span>}{b.exception_type && <Badge tone="err">{b.exception_type}</Badge>}
            <span className="text-muted">{Math.round(100 * b.duration_ms / Math.max(1, e.duration_ms))}% of the fetch was the server</span>
          </div>
        ) : <div className="ml-16 text-[11px] text-muted">no backend span (origin not in data-propagate, or CORS blocks traceparent)</div>}
      </li>
    );
  }
  return <li className="text-xs flex items-center gap-2"><span className="text-muted font-mono w-16 shrink-0">{relText}</span><Badge>{t ?? "event"}</Badge><span className="font-mono">{e.name}</span></li>;
}
