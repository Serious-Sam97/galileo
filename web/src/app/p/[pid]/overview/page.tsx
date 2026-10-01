"use client";

import Link from "next/link";
import { useMemo } from "react";
import clsx from "clsx";
import { ArrowRight, Bug, Database, Flame, Timer } from "lucide-react";
import { useProjectId, useProjectQuery, useRunQuery } from "@/lib/hooks";
import { Card, Stat, Table, Th, Td, Empty, Badge, Skeleton } from "@/components/ui";
import { fmtMs, fmtNum, fmtUsd, ago, encodeQ, fmtDuration } from "@/lib/format";
import type { NPlusOne, Query, QueryResponse } from "@/lib/types";
import { useLastSeconds } from "@/lib/time-range";
import { useStored } from "@/lib/stored";
import { useT } from "@/lib/i18n";
import { C } from "@/lib/palette";

interface ServiceRow { service_name: string; spans: number; requests: number; errors: number; p50_ms: number; p95_ms: number; p99_ms: number; last_seen: string; llm_calls: number; llm_cost_usd: number }

/** Routes load balancers and orchestrators poll: they say nothing about users. */
const HEALTH_ROUTE = /(^|\/)(health|healthz|healthcheck|ready|readyz|live|livez|ping|status)\/?$/i;
const HIDE_KEY = "galileo.hide-health";

interface Attention { key: string; tone: string; icon: typeof Bug; kind: string; title: string; why: string; cta: string; href: string }

export default function OverviewPage() {
  const pid = useProjectId();
  const t = useT();
  const [last] = useLastSeconds();
  const [hideStored, setHideStored] = useStored(`${HIDE_KEY}.${pid}`, "1");
  const hideHealth = hideStored !== "0";
  const toggleHealth = () => setHideStored(hideHealth ? "0" : "1");

  const range = { last_seconds: last };
  // Requests: root spans a server handled. Jobs and other internal roots are not traffic.
  const root = [{ field: "is_root", op: "eq" as const, value: 1 }, { field: "kind", op: "eq" as const, value: "server" }];
  // Every route and method with its traffic, latency and error share. Also tells the noise apart:
  // health checks (by route) and CORS preflights (OPTIONS, answered before routing, so no route).
  const routesQ: Query = { dataset: "spans", time_range: range, calculations: [{ op: "COUNT" }, { op: "P95", field: "duration_ms" }, { op: "AVG", field: "is_error" }], filters: root, breakdowns: ["http_route", "http_method"], orders: [{ field: "COUNT", direction: "desc" }], limit: 300 };
  const routes = useRunQuery(routesQ, { refetchInterval: 30_000 });
  const healthRoutes = useMemo(() => [...new Set((routes.data?.groups ?? []).map((g) => g.key[0]).filter((r) => r && HEALTH_ROUTE.test(r)))], [routes.data]);
  const isNoise = (g: { key: string[] }) => healthRoutes.includes(g.key[0]) || g.key[1] === "OPTIONS";
  const noise = useMemo(() => {
    const gs = routes.data?.groups ?? [];
    const total = gs.reduce((a, g) => a + (g.totals[0] ?? 0), 0);
    const n = gs.filter(isNoise).reduce((a, g) => a + (g.totals[0] ?? 0), 0);
    return { any: n > 0, share: total ? n / total : 0 };
  }, [routes.data, healthRoutes]); // eslint-disable-line react-hooks/exhaustive-deps
  const filters: Query["filters"] = hideHealth
    ? [...root, ...(healthRoutes.length ? [{ field: "http_route", op: "not_in" as const, value: healthRoutes }] : []), { field: "http_method", op: "ne" as const, value: "OPTIONS" }]
    : root;

  const kpiQ: Query = { dataset: "spans", time_range: range, calculations: [{ op: "COUNT" }, { op: "AVG", field: "is_error" }, { op: "P95", field: "duration_ms" }], filters, breakdowns: [], orders: [], limit: 1 };
  const kpi = useRunQuery(routes.data ? kpiQ : null, { refetchInterval: 30_000 });
  const slowestQ: Query = { dataset: "spans", time_range: range, calculations: [], filters, breakdowns: [], orders: [{ field: "duration_ms", direction: "desc" }], limit: 1, columns: ["trace_id", "name", "duration_ms", "timestamp"] };
  const slowest = useRunQuery(routes.data ? slowestQ : null, { refetchInterval: 60_000 });
  const services = useProjectQuery<{ services: ServiceRow[] }>(["services", last], `/services?last_seconds=${last}`, { refetchInterval: 30_000 });
  const nplus = useProjectQuery<{ candidates: NPlusOne[] }>(["nplusone", last], `/db/nplusone?last_seconds=${last}`, { refetchInterval: 60_000 });
  const issueCounts = useProjectQuery<{ counts: Record<string, number> }>(["issue-counts"], "/issues?status=open&last_seconds=60", { refetchInterval: 60_000 });

  const total = kpi.data?.groups[0];
  const requests = total?.totals[0] ?? 0;
  const errRate = total?.totals[1] ?? 0;
  const errors = Math.round(requests * errRate);
  const p95 = total?.totals[2] ?? null;
  const series = (i: number) => (total?.series ?? []).map((p) => p.values[i] ?? 0);
  const openIssues = issueCounts.data?.counts?.open ?? 0;
  const svc = services.data?.services ?? [];
  const llmCost = svc.reduce((a, r) => a + r.llm_cost_usd, 0);
  const llmCalls = svc.reduce((a, r) => a + r.llm_calls, 0);
  const projectName = svc[0]?.service_name;

  const routeRows = (routes.data?.groups ?? []).filter((g) => !(hideHealth && isNoise(g)));
  const worstRoute = [...routeRows].filter((g) => (g.totals[2] ?? 0) > 0).sort((a, b) => (b.totals[2] ?? 0) - (a.totals[2] ?? 0))[0];
  const slowRow = slowest.data?.raw?.rows[0];
  const slowCols = slowest.data?.raw?.columns ?? [];
  const slow = slowRow ? { trace: String(slowRow[slowCols.indexOf("trace_id")]), name: String(slowRow[slowCols.indexOf("name")]), ms: Number(slowRow[slowCols.indexOf("duration_ms")]) } : null;

  const attention: Attention[] = [];
  if (slow && slow.ms >= 1000) attention.push({ key: "slow", tone: C.warn, icon: Timer, kind: "Slow request", title: `${slow.name} took ${fmtMs(slow.ms)}`, why: "The slowest request in this window. The trace shows which span held it.", cta: "Open trace", href: `/p/${pid}/traces/${slow.trace}` });
  if (errors > 0) attention.push({ key: "errors", tone: C.err, icon: Flame, kind: "Errors", title: `${fmtNum(errors)} failed request${errors === 1 ? "" : "s"}`, why: worstRoute && (worstRoute.totals[2] ?? 0) > 0 ? `Most on ${worstRoute.key[1] ?? ""} ${worstRoute.key[0] || "(no route)"}: ${((worstRoute.totals[2] ?? 0) * 100).toFixed(1)}% of its requests.` : "Server errors in this window.", cta: "See errors", href: `/p/${pid}/query?q=${encodeQ({ ...routesQ, filters: [...filters, { field: "is_error", op: "eq", value: 1 }], orders: [{ field: "COUNT", direction: "desc" }] })}` });
  if (openIssues > 0) attention.push({ key: "issues", tone: C.cyan, icon: Bug, kind: "Issues", title: `${openIssues} open issue${openIssues === 1 ? "" : "s"}`, why: "Repeats are already grouped, so this is the whole list to triage.", cta: "Triage", href: `/p/${pid}/issues` });
  if (nplus.data?.candidates.length) attention.push({ key: "nplus", tone: C.lilac, icon: Database, kind: "Repeated queries", title: `${nplus.data.candidates.length} N+1 pattern${nplus.data.candidates.length === 1 ? "" : "s"}`, why: `Worst: ${nplus.data.candidates[0].avg_repeats}× per request from ${nplus.data.candidates[0].function || "unknown code"}.`, cta: "Open sample", href: `/p/${pid}/traces/${nplus.data.candidates[0].sample_trace}` });

  const loading = routes.isLoading || (routes.data && kpi.isLoading);
  const noData = routes.data && (routes.data.groups.length === 0);
  const healthy = requests > 0 && errRate < 0.05;

  return (
    <div className="mx-auto max-w-[1400px] space-y-5">
      <section className="hero-vapor rise relative overflow-hidden rounded-2xl border px-6 py-6 md:px-7">
        <div className="vapor-grid pointer-events-none absolute inset-x-0 bottom-0 h-16 opacity-30" aria-hidden="true" />
        <div className="relative space-y-2">
          <div className="text-[12px] uppercase tracking-[0.14em] text-lilac">{t("Overview")} · {fmtDuration(last)}</div>
          {loading ? <Skeleton className="h-9 w-2/3" /> : noData ? (
            <h1 className="text-3xl font-bold tracking-tight">No traffic yet.</h1>
          ) : (
            <h1 className="text-[28px] md:text-[32px] font-bold leading-tight tracking-tight">
              {projectName ?? "Everything"} is {healthy ? "up" : requests ? "struggling" : "quiet"}.{" "}
              {attention.length ? <span className="text-vapor">{attention.length} thing{attention.length === 1 ? "" : "s"} want{attention.length === 1 ? "s" : ""} you.</span> : <span className="text-ok">{t("All clear")}.</span>}
            </h1>
          )}
          {!loading && !noData && (
            <p className="max-w-3xl text-[14px] leading-relaxed text-muted">
              {fmtNum(requests)} request{requests === 1 ? "" : "s"}, {errors ? `${fmtNum(errors)} failed` : "none failed"}{p95 != null ? `, p95 ${fmtMs(p95)}` : ""}.
              {noise.any && (
                <> {hideHealth ? "Health checks and CORS preflights" : "Including health checks and CORS preflights"} ({Math.round(noise.share * 100)}% of traffic){hideHealth ? " are hidden." : "."}{" "}
                  <button onClick={toggleHealth} className="text-lilac underline decoration-dotted underline-offset-4 hover:text-fg">{hideHealth ? "Show them" : "Hide them"}</button></>
              )}
            </p>
          )}
          {noData && <p className="text-muted"><Link href={`/p/${pid}/welcome`} className="text-accent underline underline-offset-4">Get started</Link>: pick your stack, get a key and see the first trace in minutes.</p>}
        </div>
      </section>

      {attention.length > 0 && (
        <section aria-label={t("Needs attention")} className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
          {attention.map((a) => {
            const Icon = a.icon;
            return (
              <article key={a.key} className="rise card-hover flex min-h-[176px] flex-col gap-2 rounded-2xl border p-4" style={{ background: `linear-gradient(180deg, color-mix(in srgb, ${a.tone} 9%, transparent), var(--panel) 65%)`, borderColor: `color-mix(in srgb, ${a.tone} 30%, var(--border))` }}>
                <div className="flex items-center gap-2 text-[11.5px] font-semibold uppercase tracking-[0.1em]" style={{ color: a.tone }}>
                  <Icon size={14} /> {a.kind}
                </div>
                <h2 className="text-[16px] font-semibold leading-snug">{a.title}</h2>
                <p className="flex-1 text-[13px] leading-relaxed text-muted">{a.why}</p>
                <Link href={a.href} className="inline-flex items-center gap-1 self-start rounded-lg border px-3 py-1.5 text-[13px] font-semibold hover:bg-panel-2" style={{ color: a.tone, borderColor: `color-mix(in srgb, ${a.tone} 45%, transparent)` }}>{a.cta} <ArrowRight size={14} /></Link>
              </article>
            );
          })}
        </section>
      )}

      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        {loading ? [0, 1, 2, 3].map((i) => <Skeleton key={i} className="h-[104px]" />) : <>
          <Stat label={t("Requests")} value={fmtNum(requests)} sub={hideHealth && noise.any ? "health checks and preflights hidden" : "root spans"} spark={series(0)} sparkColor={C.accent} />
          <Stat label={t("Error rate")} value={requests ? (errRate * 100).toFixed(2) + "%" : "–"} tone={errRate > 0.05 ? "err" : undefined} sub={`${fmtNum(errors)} failed`} spark={series(1)} sparkColor={C.err} />
          <Stat label={t("p95 latency")} value={p95 != null ? fmtMs(p95) : "–"} sub="per bucket below" spark={series(2)} sparkColor={C.lilac} />
          {llmCalls > 0
            ? <Stat label={t("LLM spend")} value={fmtUsd(llmCost)} sub={`${fmtNum(llmCalls)} calls`} />
            : <Stat label={t("Open issues")} value={openIssues} tone={openIssues ? "warn" : undefined} sub="across the project" />}
        </>}
      </div>

      <Card title={<span className="flex items-center gap-2">{t("Routes")} <span className="font-normal text-faint">by traffic</span></span>} actions={<Link href={`/p/${pid}/query?q=${encodeQ({ ...routesQ, filters })}`} className="text-[12.5px] text-lilac hover:text-fg">{t("Open in Query")} →</Link>}>
        {routes.isLoading ? <div className="space-y-2">{[0, 1, 2, 3, 4].map((i) => <Skeleton key={i} className="h-8" />)}</div> : routeRows.length === 0 ? <Empty>No requests in this window.</Empty> : (
          <RouteTable rows={routeRows.slice(0, 12)} pid={pid} />
        )}
      </Card>

      {!!nplus.data?.candidates.length && (
        <Card title="Repeated queries (N+1 candidates)">
          <Table className="max-h-72">
            <thead><tr><Th>Statement</Th><Th>Called from</Th><Th>Route</Th><Th className="text-right">Traces</Th><Th className="text-right">Avg repeats</Th><Th className="text-right">Avg DB time</Th><Th></Th></tr></thead>
            <tbody>{nplus.data.candidates.map((c, i) => (
              <tr key={i} className="hover:bg-panel-2/70">
                <Td className="font-mono text-[11px] max-w-[420px] truncate" title={c.statement}>{c.statement}</Td>
                <Td className="font-mono text-[11px]">{c.namespace ? `${c.namespace}.` : ""}{c.function || <span className="text-muted">(unknown)</span>}<div className="text-[10px] text-faint">{c.file}</div></Td>
                <Td className="font-mono text-[11px] text-muted">{c.sample_route}{c.routes > 1 ? ` +${c.routes - 1}` : ""}</Td>
                <Td className="text-right tabular-nums">{fmtNum(c.traces)}</Td>
                <Td className="text-right tabular-nums">{c.avg_repeats}× (max {c.max_repeats})</Td>
                <Td className="text-right font-mono">{fmtMs(c.avg_ms_per_trace)}</Td>
                <Td><Link href={`/p/${pid}/traces/${c.sample_trace}`} className="text-cyan hover:underline text-[11px]">sample trace</Link></Td>
              </tr>
            ))}</tbody>
          </Table>
        </Card>
      )}

      {svc.length > 1 && (
        <Card title={t("Services")}>
          <Table>
            <thead><tr><Th>Service</Th><Th className="text-right">Requests</Th><Th className="text-right">Errors</Th><Th className="text-right">p50</Th><Th className="text-right">p95</Th><Th className="text-right">p99</Th><Th className="text-right">LLM cost</Th><Th>Last seen</Th></tr></thead>
            <tbody>
              {svc.map((r) => {
                const errPct = r.requests ? r.errors / r.requests : 0;
                return (
                  <tr key={r.service_name} className="hover:bg-panel-2/70">
                    <Td className="font-medium">{r.service_name}</Td>
                    <Td className="text-right tabular-nums">{fmtNum(r.requests)}</Td>
                    <Td className="text-right tabular-nums">{r.errors > 0 ? <Badge tone={errPct > 0.05 ? "err" : "warn"}>{r.errors} ({(errPct * 100).toFixed(1)}%)</Badge> : <span className="text-faint">0</span>}</Td>
                    <Td className="text-right tabular-nums font-mono">{fmtMs(r.p50_ms)}</Td>
                    <Td className="text-right tabular-nums font-mono">{fmtMs(r.p95_ms)}</Td>
                    <Td className="text-right tabular-nums font-mono">{fmtMs(r.p99_ms)}</Td>
                    <Td className="text-right tabular-nums">{fmtUsd(r.llm_cost_usd)}</Td>
                    <Td className="text-muted">{ago(r.last_seen)}</Td>
                  </tr>
                );
              })}
            </tbody>
          </Table>
        </Card>
      )}
    </div>
  );
}

function RouteTable({ rows, pid }: { rows: QueryResponse["groups"]; pid: string }) {
  const maxP95 = Math.max(1, ...rows.map((g) => g.totals[1] ?? 0));
  return (
    <div className="overflow-x-auto">
      <table className="w-full min-w-[640px] text-left text-[13px]">
        <thead>
          <tr className="text-[11px] uppercase tracking-[0.06em] text-faint">
            <th className="pb-2 font-semibold">Route</th><th className="pb-2 text-right font-semibold">Requests</th><th className="pb-2 text-right font-semibold">Errors</th><th className="pb-2 text-right font-semibold">p95</th><th className="w-[30%] pb-2 pl-6 font-semibold">Latency</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((g) => {
            const [route, method] = [g.key[0] ?? "", g.key[1] ?? ""];
            const p95 = g.totals[1] ?? 0;
            const err = g.totals[2] ?? 0;
            const href = `/p/${pid}/traces?${route ? `route=${encodeURIComponent(route)}` : "noroute=1"}${method ? `&method=${encodeURIComponent(method)}` : ""}`;
            return (
              <tr key={`${method} ${route}`} className="border-t border-border/60 hover:bg-panel-2/60">
                <td className="py-2.5 pr-3">
                  <Link href={href} className="group inline-flex items-baseline gap-2 font-mono text-[12.5px]" title={route ? undefined : "Requests no route matched: 404s, and requests answered by middleware before routing"}>
                    <span className="w-14 shrink-0 text-[11px] font-semibold text-lilac">{method}</span>
                    <span className={clsx("group-hover:text-accent", !route && "italic text-muted")}>{route || "(no route)"}</span>
                  </Link>
                </td>
                <td className="py-2.5 text-right tabular-nums">{fmtNum(g.totals[0] ?? 0)}</td>
                <td className={clsx("py-2.5 text-right tabular-nums", err > 0 ? "text-err" : "text-faint")}>{err > 0 ? `${(err * 100).toFixed(1)}%` : "0"}</td>
                <td className="py-2.5 text-right font-mono tabular-nums">{fmtMs(p95)}</td>
                <td className="py-2.5 pl-6">
                  <span className="block h-2 rounded-full bg-panel-3">
                    <span className="block h-2 rounded-full bg-gradient-to-r from-accent-2 to-accent shadow-[0_0_10px_rgb(255_92_207/0.35)]" style={{ width: `${Math.max(2, (p95 / maxP95) * 100)}%` }} />
                  </span>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}
