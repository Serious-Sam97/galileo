"use client";

import { useMemo, useState } from "react";
import Link from "next/link";
import { useProjectId, useProjectQuery, useProjectMutation, useRunQuery } from "@/lib/hooks";
import { post, put, del, patch } from "@/lib/api";
import { Button, Card, Stat, Table, Th, Td, Empty, ErrorBox, Badge, Drawer, Input, Label, Select, Textarea, PageHeader, Tabs } from "@/components/ui";
import { Chart, axisStyle, type EChartsOption } from "@/components/charts/chart";
import { fmtNum, fmtUsd, fmtMs, fmtTime, colorFor } from "@/lib/format";
import type { Provider, Route, PromptSummary, PromptVersion, Query, EvalsRes, SpanQuality, QualityAgg, AgentRun, GoldenDataset, PromptCi, Guardrails } from "@/lib/types";
import { RecipientsEditor } from "@/components/recipients";
import { useEffect } from "react";
import { Plus, Trash2, Pencil } from "lucide-react";
import { C, tooltipStyle } from "@/lib/palette";
import { useLastSeconds } from "@/lib/time-range";

type Tab = "usage" | "calls" | "agents" | "routes" | "providers" | "prompts" | "datasets" | "evals";

export default function AiPage() {
  const [tab, setTab] = useState<Tab>("usage");
  return (
    <div className="mx-auto max-w-[1400px] space-y-4">
      <PageHeader title="AI" sub="Gateway usage and cost, every model call, agent runs, routes, providers, prompts, datasets and evals." />
      <Tabs tabs={["usage", "calls", "agents", "routes", "providers", "prompts", "datasets", "evals"] as Tab[]} value={tab} onChange={setTab} />
      {tab === "usage" && <Usage />}
      {tab === "calls" && <Calls />}
      {tab === "routes" && <Routes />}
      {tab === "providers" && <Providers />}
      {tab === "prompts" && <Prompts />}
      {tab === "evals" && <Evals />}
      {tab === "agents" && <Agents />}
      {tab === "datasets" && <Datasets />}
    </div>
  );
}

interface UsageRes {
  totals: { calls: number; cost_usd: number; input_tokens: number; output_tokens: number; errors: number; traces: number } | null;
  by_model: { model: string; system: string; calls: number; input_tokens: number; output_tokens: number; cost_usd: number; p50_ms: number; p95_ms: number; errors: number }[];
  by_route: { route: string; calls: number; tokens: number; cost_usd: number; fallbacks: number; errors: number }[];
  by_day: { day: string; model: string; calls: number; cost_usd: number; tokens: number }[];
}

function Usage() {
  const [days, setDays] = useState(7);
  const u = useProjectQuery<UsageRes>(["ai-usage", days], `/gateway/usage?last_seconds=${days * 86400}`);
  const ev = useProjectQuery<EvalsRes>(["ai-evals"], "/gateway/evals");
  const score = (list: QualityAgg[] | undefined, key: string) => { const a = list?.find((x) => x.key === key); return a?.avg != null ? <span title={`${a.n} judged`}>{a.avg.toFixed(2)}</span> : <span className="text-muted">–</span>; };
  const d = u.data;
  const option = useMemo<EChartsOption>(() => {
    if (!d) return {};
    const models = [...new Set(d.by_day.map((r) => r.model))];
    const dayList = [...new Set(d.by_day.map((r) => r.day))].sort();
    return {
      tooltip: { trigger: "axis", ...tooltipStyle, valueFormatter: (v) => fmtUsd(v as number) },
      legend: { top: 0, textStyle: { color: C.faint, fontSize: 10 } },
      xAxis: { type: "category", data: dayList, ...axisStyle },
      yAxis: { type: "value", ...axisStyle, axisLabel: { ...axisStyle.axisLabel, formatter: (v: number) => fmtUsd(v) } },
      series: models.map((m, i) => ({ name: m, type: "bar", stack: "cost", itemStyle: { color: colorFor(i) }, data: dayList.map((day) => d.by_day.find((r) => r.day === day && r.model === m)?.cost_usd ?? 0) })),
    };
  }, [d]);
  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2"><Select value={days} onChange={(e) => setDays(Number(e.target.value))}>{[1, 7, 30].map((x) => <option key={x} value={x}>Last {x}d</option>)}</Select></div>
      <ErrorBox error={u.error} />
      {d?.totals && (
        <div className="grid grid-cols-2 gap-3 md:grid-cols-5">
          <Stat label="Calls" value={fmtNum(d.totals.calls)} sub={`${d.totals.traces} traces`} />
          <Stat label="Cost" value={fmtUsd(d.totals.cost_usd)} />
          <Stat label="Input tokens" value={fmtNum(d.totals.input_tokens)} />
          <Stat label="Output tokens" value={fmtNum(d.totals.output_tokens)} />
          <Stat label="Errors" value={d.totals.errors} tone={d.totals.errors ? "err" : undefined} />
        </div>
      )}
      {d && d.by_day.length > 0 && <Card title="Cost per day by model"><Chart option={option} height={220} /></Card>}
      <div className="grid gap-3 md:grid-cols-2">
        <Card title="By model">
          {d?.by_model.length ? (
            <Table><thead><tr><Th>Model</Th><Th className="text-right">Calls</Th><Th className="text-right">In</Th><Th className="text-right">Out</Th><Th className="text-right">Cost</Th><Th className="text-right">p95</Th><Th className="text-right">Err</Th><Th className="text-right" title="LLM-judge score 1–5">Score</Th></tr></thead>
              <tbody>{d.by_model.map((r) => <tr key={r.model + r.system}><Td className="font-mono">{r.model} <span className="text-muted">{r.system}</span></Td><Td className="text-right">{fmtNum(r.calls)}</Td><Td className="text-right">{fmtNum(r.input_tokens)}</Td><Td className="text-right">{fmtNum(r.output_tokens)}</Td><Td className="text-right">{fmtUsd(r.cost_usd)}</Td><Td className="text-right font-mono">{fmtMs(r.p95_ms)}</Td><Td className="text-right">{r.errors}</Td><Td className="text-right tabular-nums">{score(ev.data?.by_model, r.model)}</Td></tr>)}</tbody></Table>
          ) : <Empty>No LLM calls yet.</Empty>}
        </Card>
        <Card title="By route">
          {d?.by_route.length ? (
            <Table><thead><tr><Th>Route</Th><Th className="text-right">Calls</Th><Th className="text-right">Tokens</Th><Th className="text-right">Cost</Th><Th className="text-right">Fallbacks</Th><Th className="text-right">Err</Th><Th className="text-right" title="LLM-judge score 1–5">Score</Th></tr></thead>
              <tbody>{d.by_route.map((r) => <tr key={r.route}><Td className="font-mono">{r.route || <span className="text-muted">(external)</span>}</Td><Td className="text-right">{fmtNum(r.calls)}</Td><Td className="text-right">{fmtNum(r.tokens)}</Td><Td className="text-right">{fmtUsd(r.cost_usd)}</Td><Td className="text-right">{r.fallbacks}</Td><Td className="text-right">{r.errors}</Td><Td className="text-right tabular-nums">{score(ev.data?.by_route, r.route)}</Td></tr>)}</tbody></Table>
          ) : <Empty>No routed calls yet.</Empty>}
        </Card>
      </div>
      {ev.data && (ev.data.by_prompt.length > 0 || ev.data.feedback?.n) ? (
        <div className="grid gap-3 md:grid-cols-2">
          <Card title="Quality by prompt version (LLM judge)">
            {ev.data.by_prompt.length === 0 ? <Empty>No judged calls carry a prompt version yet.</Empty> : (
              <Table><thead><tr><Th>Prompt @ version</Th><Th className="text-right">Judged</Th><Th className="text-right">Avg score</Th></tr></thead>
                <tbody>{ev.data.by_prompt.map((r) => <tr key={r.key}><Td className="font-mono">{r.key}</Td><Td className="text-right">{r.n}</Td><Td className="text-right tabular-nums">{r.avg?.toFixed(2) ?? "–"}</Td></tr>)}</tbody></Table>
            )}
          </Card>
          <Card title="User feedback">
            {ev.data.feedback?.n ? <div className="grid grid-cols-2 gap-3"><Stat label="Ratings" value={fmtNum(ev.data.feedback.n)} /><Stat label="Positive share" value={`${Math.round((ev.data.feedback.avg ?? 0) * 100)}%`} /></div> : <Empty>No feedback yet. Send POST /gw/v1/feedback from the app or rate calls in the Calls tab.</Empty>}
          </Card>
        </div>
      ) : null}
    </div>
  );
}

function Calls() {
  const pid = useProjectId();
  const [last] = useLastSeconds();
  const [model, setModel] = useState("");
  const [sel, setSel] = useState<Record<string, unknown> | null>(null);
  const q: Query = { dataset: "spans", time_range: { last_seconds: last }, calculations: [], filters: [{ field: "gen_ai.system", op: "exists" }, ...(model ? [{ field: "gen_ai.response.model", op: "eq" as const, value: model }] : [])], breakdowns: [], orders: [], limit: 200, columns: ["gen_ai.usage.input_tokens", "gen_ai.usage.output_tokens", "gen_ai.usage.cost_usd", "gen_ai.galileo.route", "gen_ai.response.finish_reasons", "gen_ai.prompt", "gen_ai.completion", "gen_ai.galileo.time_to_first_token_ms", "gen_ai.galileo.cache_hit", "gen_ai.galileo.experiment", "gen_ai.galileo.experiment.arm", "gen_ai.galileo.prompt.name", "gen_ai.galileo.prompt.version", "gen_ai.tool_calls", "gen_ai.galileo.guardrail", "gen_ai.galileo.cache_kind", "gen_ai.galileo.routing", "gen_ai.conversation.id"] };
  const r = useRunQuery(q);
  const cols = r.data?.raw?.columns ?? [];
  const ix = (c: string) => cols.indexOf(c);
  const spanIds = useMemo(() => (r.data?.raw?.rows ?? []).map((row) => String(row[ix("span_id")] ?? "")).filter(Boolean), [r.data]); // eslint-disable-line react-hooks/exhaustive-deps
  const [quality, setQuality] = useState<SpanQuality>({});
  useEffect(() => { if (!spanIds.length) return; post<{ quality: SpanQuality }>(`/api/projects/${pid}/gateway/quality`, { span_ids: spanIds }).then((q) => setQuality(q.quality)).catch(() => {}); }, [spanIds, pid]);
  const [comment, setComment] = useState("");
  const fb = useProjectMutation<{ span_id: string; trace_id: string; rating: number; comment: string }>((p, b) => post(`/api/projects/${p}/gateway/feedback`, b), [["ai-evals"]]);
  const rate = async (rating: number) => { if (!sel) return; await fb.mutateAsync({ span_id: String(sel.span_id), trace_id: String(sel.trace_id), rating, comment }); setComment(""); const q = await post<{ quality: SpanQuality }>(`/api/projects/${pid}/gateway/quality`, { span_ids: [String(sel.span_id)] }); setQuality((old) => ({ ...old, ...q.quality })); };
  const qual = sel ? quality[String(sel.span_id)] : undefined;
  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <Input className="w-56" placeholder="model" value={model} onChange={(e) => setModel(e.target.value)} />
      </div>
      <ErrorBox error={r.error} />
      {r.data?.raw && (r.data.raw.rows.length === 0 ? <Empty>No LLM calls in this window.</Empty> : (
        <Table className="max-h-[calc(100vh-220px)]">
          <thead><tr><Th>Time</Th><Th>Model</Th><Th>Route</Th><Th>Service</Th><Th className="text-right">In / Out</Th><Th className="text-right">Cost</Th><Th className="text-right">Latency</Th><Th>Finish</Th><Th>Quality</Th><Th>User</Th><Th>Trace</Th></tr></thead>
          <tbody>
            {r.data.raw.rows.map((row, i) => {
              const obj = Object.fromEntries(cols.map((c, j) => [c, row[j]]));
              const st = String(obj.status_code);
              return (
                <tr key={i} className="hover:bg-panel-2 cursor-pointer" onClick={() => setSel(obj)}>
                  <Td className="text-muted whitespace-nowrap">{fmtTime(String(obj.timestamp))}</Td>
                  <Td className="font-mono">{String(obj.gen_ai_model)}</Td>
                  <Td className="font-mono text-muted">{String(obj["gen_ai.galileo.route"] ?? "")}</Td>
                  <Td>{String(obj.service_name)}</Td>
                  <Td className="text-right tabular-nums">{fmtNum(Number(obj["gen_ai.usage.input_tokens"] ?? 0))} / {fmtNum(Number(obj["gen_ai.usage.output_tokens"] ?? 0))}</Td>
                  <Td className="text-right tabular-nums">{fmtUsd(Number(obj["gen_ai.usage.cost_usd"] ?? 0))}</Td>
                  <Td className="text-right font-mono">{fmtMs(Number(obj.duration_ms))}</Td>
                  <Td><Badge tone={st === "error" ? "err" : "muted"}>{st === "error" ? "error" : String(obj["gen_ai.response.finish_reasons"] ?? "")}</Badge>{obj["gen_ai.galileo.cache_hit"] === true || obj["gen_ai.galileo.cache_hit"] === "true" ? <Badge tone="accent" className="ml-1">{String(obj["gen_ai.galileo.cache_kind"] ?? "cache")}</Badge> : null}{obj["gen_ai.galileo.guardrail"] ? <span title={`guardrail: ${String(obj["gen_ai.galileo.guardrail"])}`}><Badge tone="warn" className="ml-1">{String(obj["gen_ai.galileo.guardrail"])}</Badge></span> : null}{obj["gen_ai.galileo.routing"] ? <Badge tone="muted" className="ml-1">health-routed</Badge> : null}</Td>
                  <Td className="whitespace-nowrap text-[11px]">{(() => { const qq = quality[String(obj.span_id)]; if (!qq) return null; const sc = qq.evals.find((e) => e.score != null)?.score; const up = qq.feedback.filter((f) => f.rating > 0 && (f.rating === 1 || f.rating >= 4)).length; const down = qq.feedback.filter((f) => f.rating === -1 || (f.rating > 1 && f.rating <= 2)).length; return <>{sc != null && <Badge tone={sc >= 4 ? "ok" : sc <= 2 ? "err" : "muted"}>judge {sc}</Badge>} {up ? <span className="text-ok">👍{up}</span> : null} {down ? <span className="text-err">👎{down}</span> : null}</>; })()}</Td>
                  <Td className="font-mono text-muted">{String(obj.user_id ?? "")}</Td>
                  <Td><Link href={`/p/${pid}/traces/${obj.trace_id}`} onClick={(e) => e.stopPropagation()} className="font-mono text-info hover:underline">{String(obj.trace_id).slice(0, 8)}…</Link></Td>
                </tr>
              );
            })}
          </tbody>
        </Table>
      ))}
      <Drawer open={!!sel} onClose={() => setSel(null)} title={sel ? `chat ${sel.gen_ai_model}` : ""} width="w-[720px]">
        {sel && (
          <div className="space-y-3 text-sm">
            <div className="flex flex-wrap gap-2">
              <Badge>{fmtMs(Number(sel.duration_ms))}</Badge>
              {sel["gen_ai.galileo.time_to_first_token_ms"] ? <Badge>TTFT {fmtMs(Number(sel["gen_ai.galileo.time_to_first_token_ms"]))}</Badge> : null}
              <Badge>{fmtNum(Number(sel["gen_ai.usage.input_tokens"] ?? 0))} in</Badge><Badge>{fmtNum(Number(sel["gen_ai.usage.output_tokens"] ?? 0))} out</Badge><Badge tone="accent">{fmtUsd(Number(sel["gen_ai.usage.cost_usd"] ?? 0))}</Badge>
            </div>
            <div><Label>Prompt</Label><pre className="whitespace-pre-wrap rounded border bg-bg p-2 font-mono text-[11px] max-h-72 overflow-auto scroll-thin">{String(sel["gen_ai.prompt"] ?? "(not recorded)")}</pre></div>
            <div><Label>Completion</Label><pre className="whitespace-pre-wrap rounded border bg-bg p-2 font-mono text-[11px] max-h-72 overflow-auto scroll-thin">{String(sel["gen_ai.completion"] ?? "(not recorded)")}</pre></div>
            {sel["gen_ai.tool_calls"] ? <div><Label>Tool calls</Label><pre className="whitespace-pre-wrap rounded border bg-bg p-2 font-mono text-[11px] max-h-40 overflow-auto scroll-thin">{String(sel["gen_ai.tool_calls"])}</pre></div> : null}
            <div className="flex flex-wrap gap-2 text-[11px] text-muted">
              {sel["gen_ai.galileo.prompt.name"] ? <Badge>prompt {String(sel["gen_ai.galileo.prompt.name"])}@{String(sel["gen_ai.galileo.prompt.version"] ?? "?")}</Badge> : null}
              {sel["gen_ai.galileo.experiment"] ? <Badge tone="accent">experiment {String(sel["gen_ai.galileo.experiment"])} · arm {String(sel["gen_ai.galileo.experiment.arm"])}</Badge> : null}
              {sel["gen_ai.galileo.cache_hit"] === true || sel["gen_ai.galileo.cache_hit"] === "true" ? <Badge tone="accent">served from {String(sel["gen_ai.galileo.cache_kind"] ?? "")} cache</Badge> : null}
              {sel["gen_ai.galileo.guardrail"] ? <Badge tone="warn">guardrail {String(sel["gen_ai.galileo.guardrail"])}</Badge> : null}
              {sel["gen_ai.conversation.id"] ? <Link href={`/p/${pid}/ai/agents/${String(sel["gen_ai.conversation.id"])}`} className="text-info hover:underline">agent run →</Link> : null}
            </div>
            <AddToDataset spanId={String(sel.span_id)} />
            <div className="rounded border p-2 space-y-2">
              <Label>Feedback</Label>
              {qual?.evals.length ? <div className="text-xs">Judge: {qual.evals.map((e, i) => <span key={i} className="mr-2"><b>{e.score ?? "?"}</b>/5 <span className="text-muted">{e.reasoning}</span></span>)}</div> : null}
              {qual?.feedback.length ? <ul className="text-xs space-y-0.5">{qual.feedback.map((f, i) => <li key={i}>{f.rating === -1 || (f.rating > 1 && f.rating <= 2) ? "👎" : "👍"} <span className="text-muted">{f.user}</span> {f.comment}</li>)}</ul> : <p className="text-xs text-muted">No feedback for this call yet.</p>}
              <div className="flex gap-2 items-center">
                <Input className="flex-1" placeholder="comment (optional)" value={comment} onChange={(e) => setComment(e.target.value)} />
                <Button size="sm" onClick={() => rate(1)}>👍 Good</Button>
                <Button size="sm" onClick={() => rate(-1)}>👎 Bad</Button>
              </div>
              <ErrorBox error={fb.error} />
            </div>
            <Link href={`/p/${pid}/traces/${sel.trace_id}`} className="text-info hover:underline">Open trace →</Link>
          </div>
        )}
      </Drawer>
    </div>
  );
}

function Providers() {
  const list = useProjectQuery<{ providers: Provider[] }>(["providers"], "/gateway/providers");
  const [edit, setEdit] = useState<Partial<Provider> & { api_key?: string } | null>(null);
  const save = useProjectMutation<Partial<Provider> & { api_key?: string }>((p, b) => (b.id ? put(`/api/projects/${p}/gateway/providers/${b.id}`, b) : post(`/api/projects/${p}/gateway/providers`, b)), [["providers"]]);
  const remove = useProjectMutation<string>((p, id) => del(`/api/projects/${p}/gateway/providers/${id}`), [["providers"]]);
  return (
    <div className="space-y-3">
      <div className="flex justify-between items-center"><p className="text-muted text-sm">Where model calls go. Keys are encrypted at rest and never shown again.</p><Button variant="primary" size="sm" onClick={() => setEdit({ kind: "anthropic", name: "", base_url: "" })}><Plus size={13} /> Provider</Button></div>
      {list.data?.providers.length === 0 && <Empty>No providers yet. Add Anthropic, OpenAI, Ollama, or any OpenAI-compatible endpoint.</Empty>}
      {!!list.data?.providers.length && (
        <Table><thead><tr><Th>Name</Th><Th>Kind</Th><Th>Base URL</Th><Th>Key</Th><Th></Th></tr></thead>
          <tbody>{list.data.providers.map((p) => (
            <tr key={p.id}><Td className="font-medium">{p.name}</Td><Td><Badge>{p.kind}</Badge></Td><Td className="font-mono text-muted">{p.base_url}</Td><Td>{p.has_key ? <Badge tone="ok">set</Badge> : <Badge>none</Badge>}</Td>
              <Td className="text-right whitespace-nowrap"><Button size="sm" variant="ghost" onClick={() => setEdit({ ...p })}><Pencil size={12} /></Button><Button size="sm" variant="ghost" onClick={() => confirm(`Delete provider ${p.name}?`) && remove.mutate(p.id)}><Trash2 size={12} /></Button></Td></tr>
          ))}</tbody></Table>
      )}
      <Drawer open={!!edit} onClose={() => setEdit(null)} title={edit?.id ? "Edit provider" : "New provider"}>
        {edit && (
          <form className="space-y-3" onSubmit={async (e) => { e.preventDefault(); await save.mutateAsync(edit); setEdit(null); }}>
            <div><Label>Name (used as `name/model` for direct calls)</Label><Input value={edit.name ?? ""} onChange={(e) => setEdit({ ...edit, name: e.target.value })} required placeholder="anthropic" /></div>
            <div><Label>Kind</Label><Select className="w-full" value={edit.kind} onChange={(e) => setEdit({ ...edit, kind: e.target.value as Provider["kind"] })}><option value="anthropic">Anthropic</option><option value="openai">OpenAI</option><option value="ollama">Ollama</option><option value="openai_compatible">OpenAI-compatible (vLLM, Groq, …)</option></Select></div>
            <div><Label>Base URL (blank = default for kind)</Label><Input value={edit.base_url ?? ""} onChange={(e) => setEdit({ ...edit, base_url: e.target.value })} placeholder="https://api.anthropic.com" /></div>
            <div><Label>API key {edit.id && "(leave blank to keep)"}</Label><Input type="password" value={edit.api_key ?? ""} onChange={(e) => setEdit({ ...edit, api_key: e.target.value })} autoComplete="off" /></div>
            <ErrorBox error={save.error} />
            <Button type="submit" variant="primary">Save</Button>
          </form>
        )}
      </Drawer>
    </div>
  );
}

function Routes() {
  const providers = useProjectQuery<{ providers: Provider[] }>(["providers"], "/gateway/providers");
  const list = useProjectQuery<{ routes: Route[] }>(["routes"], "/gateway/routes");
  const [edit, setEdit] = useState<Partial<Route> | null>(null);
  const save = useProjectMutation<Partial<Route>>((p, b) => (b.id ? put(`/api/projects/${p}/gateway/routes/${b.id}`, b) : post(`/api/projects/${p}/gateway/routes`, b)), [["routes"]]);
  const remove = useProjectMutation<string>((p, id) => del(`/api/projects/${p}/gateway/routes/${id}`), [["routes"]]);
  const pname = (id: string) => providers.data?.providers.find((p) => p.id === id)?.name ?? "?";
  return (
    <div className="space-y-3">
      <div className="flex justify-between items-center"><p className="text-muted text-sm">Apps ask for the <b>alias</b> as the model name. Targets are tried in order; the next one is used on 429/5xx/connection errors.</p>
        <Button variant="primary" size="sm" onClick={() => setEdit({ alias: "", targets: [{ provider_id: providers.data?.providers[0]?.id ?? "", model: "" }], budget: {}, rate_limit: {}, enabled: true })}><Plus size={13} /> Route</Button></div>
      {list.data?.routes.length === 0 && <Empty>No routes yet.</Empty>}
      {!!list.data?.routes.length && (
        <Table><thead><tr><Th>Alias</Th><Th>Targets</Th><Th>Limits</Th><Th></Th><Th></Th></tr></thead>
          <tbody>{list.data.routes.map((r) => (
            <tr key={r.id}><Td className="font-mono font-medium">{r.alias}{!r.enabled && <Badge tone="warn" className="ml-2">disabled</Badge>}</Td>
              <Td>{r.targets.map((t, i) => <div key={i} className="font-mono text-[11px]">{i > 0 && <span className="text-muted">↳ </span>}{pname(t.provider_id)}/{t.model}</div>)}</Td>
              <Td className="text-muted text-[11px]">{[r.rate_limit.requests_per_minute && `${r.rate_limit.requests_per_minute} rpm`, r.budget.daily_usd && `$${r.budget.daily_usd}/day`, r.budget.monthly_usd && `$${r.budget.monthly_usd}/mo`, r.budget.daily_tokens && `${r.budget.daily_tokens} tok/day`, r.budget.record_content === false && "no content", r.budget.cache_ttl_secs && `cache ${r.budget.cache_ttl_secs}s`, r.budget.experiment && `exp ${r.budget.experiment.name}`, r.budget.guardrails && (r.budget.guardrails.pii !== "off" || r.budget.guardrails.injection !== "off" || r.budget.guardrails.denylist?.length) && "guardrails", r.budget.semantic_cache && "semantic cache", r.budget.smart_routing && "smart routing"].filter(Boolean).join(" · ") || "none"}</Td>
              <Td className="text-muted text-[11px]">{r.description}</Td>
              <Td className="text-right whitespace-nowrap"><Button size="sm" variant="ghost" onClick={() => setEdit({ ...r })}><Pencil size={12} /></Button><Button size="sm" variant="ghost" onClick={() => confirm(`Delete route ${r.alias}?`) && remove.mutate(r.id)}><Trash2 size={12} /></Button></Td></tr>
          ))}</tbody></Table>
      )}
      <Drawer open={!!edit} onClose={() => setEdit(null)} title={edit?.id ? "Edit route" : "New route"} width="w-[640px]">
        {edit && (
          <form className="space-y-3" onSubmit={async (e) => { e.preventDefault(); await save.mutateAsync(edit); setEdit(null); }}>
            <div className="grid grid-cols-2 gap-3">
              <div><Label>Alias</Label><Input value={edit.alias ?? ""} onChange={(e) => setEdit({ ...edit, alias: e.target.value })} required placeholder="assistant" className="font-mono" /></div>
              <div><Label>Description</Label><Input value={edit.description ?? ""} onChange={(e) => setEdit({ ...edit, description: e.target.value })} /></div>
            </div>
            <div>
              <Label>Targets (in fallback order)</Label>
              {(edit.targets ?? []).map((t, i) => (
                <div key={i} className="mb-1.5 flex gap-1.5">
                  <Select value={t.provider_id} onChange={(e) => setEdit({ ...edit, targets: edit.targets!.map((x, j) => (j === i ? { ...x, provider_id: e.target.value } : x)) })}>{providers.data?.providers.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}</Select>
                  <Input className="font-mono" placeholder="model id" value={t.model} onChange={(e) => setEdit({ ...edit, targets: edit.targets!.map((x, j) => (j === i ? { ...x, model: e.target.value } : x)) })} required />
                  <button type="button" className="text-muted hover:text-err" onClick={() => setEdit({ ...edit, targets: edit.targets!.filter((_, j) => j !== i) })}><Trash2 size={13} /></button>
                </div>
              ))}
              <Button type="button" size="sm" onClick={() => setEdit({ ...edit, targets: [...(edit.targets ?? []), { provider_id: providers.data?.providers[0]?.id ?? "", model: "" }] })}><Plus size={12} /> target</Button>
            </div>
            <div className="grid grid-cols-3 gap-3">
              <div><Label>Requests / minute</Label><Input type="number" value={edit.rate_limit?.requests_per_minute ?? ""} onChange={(e) => setEdit({ ...edit, rate_limit: { requests_per_minute: e.target.value ? Number(e.target.value) : undefined } })} /></div>
              <div><Label>Daily budget USD</Label><Input type="number" step="0.01" value={edit.budget?.daily_usd ?? ""} onChange={(e) => setEdit({ ...edit, budget: { ...edit.budget, daily_usd: e.target.value ? Number(e.target.value) : undefined } })} /></div>
              <div><Label>Monthly budget USD</Label><Input type="number" step="0.01" value={edit.budget?.monthly_usd ?? ""} onChange={(e) => setEdit({ ...edit, budget: { ...edit.budget, monthly_usd: e.target.value ? Number(e.target.value) : undefined } })} /></div>
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div><Label>Response cache TTL (seconds)</Label><Input type="number" placeholder="off" value={edit.budget?.cache_ttl_secs ?? ""} onChange={(e) => setEdit({ ...edit, budget: { ...edit.budget, cache_ttl_secs: e.target.value ? Number(e.target.value) : undefined } })} /><p className="text-[11px] text-muted mt-1">Exact-match cache for non-streaming calls. Hits cost $0 and carry <code>x-galileo-cache: hit</code>.</p></div>
              <div><Label>Budget alert recipients</Label><RecipientsEditor value={edit.budget?.alert_recipients ?? []} onChange={(r) => setEdit({ ...edit, budget: { ...edit.budget, alert_recipients: r } })} /><p className="text-[11px] text-muted mt-1">Notified at 80% and when exhausted. Empty = project issue recipients.</p></div>
            </div>
            <div className="rounded border p-2 space-y-2">
              <div className="text-[11px] uppercase tracking-wide text-muted">Guardrails</div>
              {(() => { const g: Guardrails = edit.budget?.guardrails ?? { pii: "off", injection: "off", denylist: [] }; const setG = (patch: Partial<Guardrails>) => setEdit({ ...edit, budget: { ...edit.budget, guardrails: { ...g, ...patch } } }); const opts = ["off", "tag", "redact", "block"] as const; return (
                <div className="grid grid-cols-3 gap-2 text-sm">
                  <div><Label>PII (e-mail, card, CPF, phone, secrets)</Label><Select value={g.pii} onChange={(e) => setG({ pii: e.target.value as Guardrails["pii"] })}>{opts.map((o) => <option key={o} value={o}>{o}</option>)}</Select></div>
                  <div><Label>Prompt injection</Label><Select value={g.injection} onChange={(e) => setG({ injection: e.target.value as Guardrails["injection"] })}>{["off", "tag", "block"].map((o) => <option key={o} value={o}>{o}</option>)}</Select></div>
                  <div><Label>Denylist (regex, one per line)</Label><Textarea rows={2} className="font-mono text-[11px]" value={g.denylist.join("\n")} onChange={(e) => setG({ denylist: e.target.value.split("\n").map((x) => x.trim()).filter(Boolean) })} /></div>
                  <div><Label>Max tokens / user / day</Label><Input type="number" value={g.max_tokens_per_user_day ?? ""} onChange={(e) => setG({ max_tokens_per_user_day: e.target.value ? Number(e.target.value) : null })} /></div>
                  <div><Label>Max USD / user / day</Label><Input type="number" step="0.01" value={g.max_cost_per_user_day ?? ""} onChange={(e) => setG({ max_cost_per_user_day: e.target.value ? Number(e.target.value) : null })} /></div>
                  <p className="text-[11px] text-muted self-end">Hits are recorded as <code>gen_ai.galileo.guardrail</code>; block answers 400 in the client&apos;s format.</p>
                </div>); })()}
            </div>
            <div className="grid grid-cols-2 gap-3">
              <div className="rounded border p-2 space-y-1">
                <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={!!edit.budget?.semantic_cache} onChange={(e) => setEdit({ ...edit, budget: { ...edit.budget, semantic_cache: e.target.checked ? { threshold: 0.92, ttl_secs: 3600 } : null } })} /> Semantic cache (near-duplicate prompts)</label>
                {edit.budget?.semantic_cache && <div className="flex gap-2 items-end"><div><Label>threshold</Label><Input type="number" step="0.01" min={0.5} max={0.999} className="w-24" value={edit.budget.semantic_cache.threshold} onChange={(e) => setEdit({ ...edit, budget: { ...edit.budget, semantic_cache: { ...edit.budget!.semantic_cache!, threshold: Number(e.target.value) } } })} /></div><div><Label>TTL s</Label><Input type="number" className="w-24" value={edit.budget.semantic_cache.ttl_secs} onChange={(e) => setEdit({ ...edit, budget: { ...edit.budget, semantic_cache: { ...edit.budget!.semantic_cache!, ttl_secs: Number(e.target.value) } } })} /></div></div>}
              </div>
              <div className="rounded border p-2"><label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={!!edit.budget?.smart_routing} onChange={(e) => setEdit({ ...edit, budget: { ...edit.budget, smart_routing: e.target.checked ? true : null } })} /> Smart routing (order targets by live health)</label><p className="text-[11px] text-muted mt-1">Targets with over 50% errors in the last 5 minutes are tried last.</p></div>
            </div>
            <div className="rounded border p-2 space-y-2">
              <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={!!edit.budget?.experiment} onChange={(e) => setEdit({ ...edit, budget: { ...edit.budget, experiment: e.target.checked ? { name: "", prompt_name: "", version_a: 1, version_b: 2, percent_b: 50, sticky: true } : null } })} /> Prompt experiment (A/B on prompt versions)</label>
              {edit.budget?.experiment && (() => { const x = edit.budget!.experiment!; const setX = (patch: Partial<typeof x>) => setEdit({ ...edit, budget: { ...edit.budget, experiment: { ...x, ...patch } } }); return (
                <div className="grid grid-cols-3 gap-2">
                  <div><Label>Experiment name</Label><Input value={x.name} onChange={(e) => setX({ name: e.target.value })} placeholder="tone-v2" /></div>
                  <div><Label>Prompt name</Label><Input className="font-mono" value={x.prompt_name} onChange={(e) => setX({ prompt_name: e.target.value })} placeholder="assistant-system" /></div>
                  <div><Label>% to B</Label><Input type="number" min={0} max={100} value={x.percent_b} onChange={(e) => setX({ percent_b: Number(e.target.value) })} /></div>
                  <div><Label>Version A</Label><Input type="number" value={x.version_a} onChange={(e) => setX({ version_a: Number(e.target.value) })} /></div>
                  <div><Label>Version B</Label><Input type="number" value={x.version_b} onChange={(e) => setX({ version_b: Number(e.target.value) })} /></div>
                  <label className="flex items-end gap-2 text-sm pb-2"><input type="checkbox" checked={x.sticky} onChange={(e) => setX({ sticky: e.target.checked })} /> Sticky per user</label>
                  <p className="col-span-3 text-[11px] text-muted">Applies when the app asks for the prompt without a version (<code>galileo.prompt.name</code> in the body, no version). The span records <code>gen_ai.galileo.experiment</code> and <code>arm</code>; compare arms in Evals.</p>
                </div>); })()}
            </div>
            <div className="flex gap-4 text-sm">
              <label className="flex items-center gap-2"><input type="checkbox" checked={edit.enabled ?? true} onChange={(e) => setEdit({ ...edit, enabled: e.target.checked })} /> Enabled</label>
              <label className="flex items-center gap-2"><input type="checkbox" checked={edit.budget?.record_content !== false} onChange={(e) => setEdit({ ...edit, budget: { ...edit.budget, record_content: e.target.checked ? undefined : false } })} /> Record prompts &amp; completions</label>
            </div>
            <ErrorBox error={save.error} />
            <Button type="submit" variant="primary">Save</Button>
          </form>
        )}
      </Drawer>
    </div>
  );
}

function Prompts() {
  const pid = useProjectId();
  const list = useProjectQuery<{ prompts: PromptSummary[] }>(["prompts"], "/gateway/prompts");
  const [open, setOpen] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const detail = useProjectQuery<{ prompt: PromptSummary; versions: PromptVersion[] }>(["prompt", open], `/gateway/prompts/${open}`, { enabled: !!open });
  const create = useProjectMutation<{ name: string; description: string; content: unknown; note: string }>((p, b) => post(`/api/projects/${p}/gateway/prompts`, b), [["prompts"]]);
  const addVersion = useProjectMutation<{ id: string; content: unknown; note: string }>((p, b) => post(`/api/projects/${p}/gateway/prompts/${b.id}/versions`, b), [["prompts"], ["prompt", open]]);
  const remove = useProjectMutation<string>((p, id) => del(`/api/projects/${p}/gateway/prompts/${id}`), [["prompts"]]);
  const [form, setForm] = useState({ name: "", description: "", system: "", messages: "", note: "" });
  const content = () => ({ system: form.system || undefined, messages: form.messages ? JSON.parse(form.messages) : [] });
  return (
    <div className="space-y-3">
      <div className="flex justify-between items-center">
        <p className="text-muted text-sm">Versioned prompts the gateway injects. Use <code className="font-mono text-fg">{"{{variable}}"}</code> placeholders and send <code className="font-mono text-fg">{`"galileo": {"prompt": {"name": "…", "variables": {…}}}`}</code> in the request body.</p>
        <Button variant="primary" size="sm" onClick={() => { setForm({ name: "", description: "", system: "", messages: "", note: "" }); setCreating(true); }}><Plus size={13} /> Prompt</Button>
      </div>
      {list.data?.prompts.length === 0 && <Empty>No prompts registered.</Empty>}
      {!!list.data?.prompts.length && (
        <Table><thead><tr><Th>Name</Th><Th>Description</Th><Th className="text-right">Latest</Th><Th className="text-right">Versions</Th><Th></Th></tr></thead>
          <tbody>{list.data.prompts.map((p) => (
            <tr key={p.id} className="hover:bg-panel-2 cursor-pointer" onClick={() => setOpen(p.id)}><Td className="font-mono font-medium">{p.name}</Td><Td className="text-muted">{p.description}</Td><Td className="text-right">v{p.latest_version ?? 0}</Td><Td className="text-right">{p.versions}</Td>
              <Td className="text-right"><Button size="sm" variant="ghost" onClick={(e) => { e.stopPropagation(); if (confirm(`Delete prompt ${p.name}?`)) remove.mutate(p.id); }}><Trash2 size={12} /></Button></Td></tr>
          ))}</tbody></Table>
      )}
      <Drawer open={creating} onClose={() => setCreating(false)} title="New prompt" width="w-[640px]">
        <PromptForm form={form} setForm={setForm} withName error={create.error} onSubmit={async () => { await create.mutateAsync({ name: form.name, description: form.description, content: content(), note: form.note }); setCreating(false); }} />
      </Drawer>
      <Drawer open={!!open} onClose={() => setOpen(null)} title={detail.data?.prompt.name ?? "Prompt"} width="w-[720px]">
        {detail.data && (
          <div className="space-y-4">
            <div className="text-xs text-muted">Call with model alias + <code className="font-mono">{`{"galileo":{"prompt":{"name":"${detail.data.prompt.name}","version":${detail.data.versions[0]?.version ?? 1}}}}`}</code> or omit version for {detail.data.prompt.promoted_version ? <>the promoted <b>v{detail.data.prompt.promoted_version}</b></> : "latest"}. Project {pid.slice(0, 8)}…</div>
            <PromptCiCard prompt={detail.data.prompt} versions={detail.data.versions} onChange={() => detail.refetch()} />
            <div><div className="text-[11px] uppercase text-muted mb-1">New version</div>
              <PromptForm form={form} setForm={setForm} error={addVersion.error} onSubmit={async () => { await addVersion.mutateAsync({ id: open!, content: content(), note: form.note }); setForm({ ...form, note: "" }); }} submitLabel="Publish version" /></div>
            <div className="space-y-2">
              {detail.data.versions.map((v) => (
                <div key={v.id} className="rounded border bg-bg p-2">
                  <div className="flex justify-between text-xs"><span className="font-medium">v{v.version} <span className="text-muted font-normal">{v.note}</span></span><span className="text-muted">{fmtTime(v.created_at)}</span></div>
                  {v.content.system && <pre className="mt-1 whitespace-pre-wrap font-mono text-[11px] text-muted">[system] {v.content.system}</pre>}
                  {v.content.messages?.map((m, i) => <pre key={i} className="whitespace-pre-wrap font-mono text-[11px] text-muted">[{m.role}] {m.content}</pre>)}
                  <button className="mt-1 text-[11px] text-info hover:underline" onClick={() => setForm({ ...form, system: v.content.system ?? "", messages: v.content.messages?.length ? JSON.stringify(v.content.messages, null, 2) : "" })}>load into editor</button>
                </div>
              ))}
            </div>
          </div>
        )}
      </Drawer>
    </div>
  );
}

function PromptForm({ form, setForm, withName, onSubmit, error, submitLabel = "Create" }: { form: { name: string; description: string; system: string; messages: string; note: string }; setForm: (f: typeof form) => void; withName?: boolean; onSubmit: () => Promise<void>; error: unknown; submitLabel?: string }) {
  return (
    <form className="space-y-3" onSubmit={async (e) => { e.preventDefault(); await onSubmit(); }}>
      {withName && <div className="grid grid-cols-2 gap-3"><div><Label>Name</Label><Input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} required className="font-mono" /></div><div><Label>Description</Label><Input value={form.description} onChange={(e) => setForm({ ...form, description: e.target.value })} /></div></div>}
      <div><Label>System prompt</Label><Textarea rows={5} value={form.system} onChange={(e) => setForm({ ...form, system: e.target.value })} placeholder="You are a helpful assistant for {{company}}." /></div>
      <div><Label>Prepended messages (JSON array, optional)</Label><Textarea rows={3} value={form.messages} onChange={(e) => setForm({ ...form, messages: e.target.value })} placeholder='[{"role":"user","content":"Context: {{context}}"}]' /></div>
      <div><Label>Version note</Label><Input value={form.note} onChange={(e) => setForm({ ...form, note: e.target.value })} /></div>
      <ErrorBox error={error} />
      <Button type="submit" variant="primary">{submitLabel}</Button>
    </form>
  );
}

// ---------------------------------------------------------------- evals

function Evals() {
  const pid = useProjectId();
  const routes = useProjectQuery<{ routes: Route[] }>(["routes"], "/gateway/routes");
  const ev = useProjectQuery<EvalsRes>(["ai-evals"], "/gateway/evals");
  const [form, setForm] = useState({ judge_route: "", sample_size: 20, rubric: "", filter_route: "", last_seconds: 86400 });
  const [result, setResult] = useState<{ sampled: number; scored: number; avg_score: number | null; error: string } | null>(null);
  const run = useProjectMutation<typeof form, { sampled: number; scored: number; avg_score: number | null; error: string }>((p, b) => post(`/api/projects/${p}/gateway/evals/run`, b), [["ai-evals"]]);
  const judge = form.judge_route || routes.data?.routes[0]?.alias || "";
  return (
    <div className="space-y-3">
      <Card title="Run an LLM-as-judge evaluation">
        <p className="text-sm text-muted mb-2">Samples recent successful completions and asks the judge route to score each 1–5 against the rubric. Scores show up per model, route and prompt version in Usage and here.</p>
        <form className="grid grid-cols-4 gap-3 items-end" onSubmit={async (e) => { e.preventDefault(); setResult(await run.mutateAsync({ ...form, judge_route: judge })); }}>
          <div><Label>Judge route</Label><Select value={judge} onChange={(e) => setForm({ ...form, judge_route: e.target.value })}>{routes.data?.routes.map((r) => <option key={r.id} value={r.alias}>{r.alias}</option>)}</Select></div>
          <div><Label>Only calls on route</Label><Select value={form.filter_route} onChange={(e) => setForm({ ...form, filter_route: e.target.value })}><option value="">any</option>{routes.data?.routes.map((r) => <option key={r.id} value={r.alias}>{r.alias}</option>)}</Select></div>
          <div><Label>Sample size</Label><Input type="number" min={1} max={100} value={form.sample_size} onChange={(e) => setForm({ ...form, sample_size: Number(e.target.value) })} /></div>
          <div><Label>Window</Label><Select value={form.last_seconds} onChange={(e) => setForm({ ...form, last_seconds: Number(e.target.value) })}>{[3600, 86400, 7 * 86400, 30 * 86400].map((s) => <option key={s} value={s}>Last {s >= 86400 ? s / 86400 + "d" : s / 3600 + "h"}</option>)}</Select></div>
          <div className="col-span-4"><Label>Rubric (optional; default grades correctness, completeness and faithfulness)</Label><Textarea rows={3} value={form.rubric} onChange={(e) => setForm({ ...form, rubric: e.target.value })} placeholder="Score 5 if the answer is in Portuguese, polite and cites the pet's name…" /></div>
          <div className="col-span-4 flex items-center gap-3"><Button type="submit" variant="primary" disabled={run.isPending || !judge}>{run.isPending ? "Judging…" : "Run evaluation"}</Button>
            {result && <span className="text-sm">{result.scored}/{result.sampled} scored{result.avg_score != null ? `, avg ${result.avg_score.toFixed(2)}` : ""}{result.error ? <span className="text-err"> · {result.error}</span> : ""}</span>}</div>
        </form>
        <ErrorBox error={run.error} />
      </Card>
      <div className="grid gap-3 md:grid-cols-3">
        {(["by_model", "by_route", "by_prompt"] as const).map((k) => (
          <Card key={k} title={{ by_model: "Score by model", by_route: "Score by route", by_prompt: "Score by prompt version" }[k]}>
            {!ev.data?.[k].length ? <Empty>Nothing judged yet.</Empty> : (
              <Table><thead><tr><Th>{k === "by_prompt" ? "Prompt" : k === "by_model" ? "Model" : "Route"}</Th><Th className="text-right">n</Th><Th className="text-right">Avg</Th></tr></thead>
                <tbody>{ev.data[k].map((r) => <tr key={r.key}><Td className="font-mono">{r.key || <span className="text-muted">(external)</span>}</Td><Td className="text-right">{r.n}</Td><Td className="text-right tabular-nums">{r.avg?.toFixed(2) ?? "–"}</Td></tr>)}</tbody></Table>
            )}
          </Card>
        ))}
      </div>
      <Card title="Runs">
        {!ev.data?.runs.length ? <Empty>No evaluation runs yet.</Empty> : (
          <Table><thead><tr><Th>When</Th><Th>Judge</Th><Th>Filter</Th><Th className="text-right">Sample</Th><Th className="text-right">Scored</Th><Th className="text-right">Avg</Th><Th>Status</Th></tr></thead>
            <tbody>{ev.data.runs.map((r) => <tr key={r.id}><Td className="text-muted whitespace-nowrap">{fmtTime(r.created_at)}</Td><Td className="font-mono">{r.judge_route}</Td><Td className="font-mono text-muted">{r.filter_route || "any"}</Td><Td className="text-right">{r.sample_size}</Td><Td className="text-right">{r.scored}</Td><Td className="text-right tabular-nums">{r.avg_score?.toFixed(2) ?? "–"}</Td><Td><Badge tone={r.status === "done" ? "ok" : r.status === "failed" ? "err" : "muted"} >{r.status}</Badge>{r.error && <span className="ml-1 text-err text-[11px]">{r.error}</span>}</Td></tr>)}</tbody></Table>
        )}
      </Card>
      <Card title="Latest judged calls">
        {!ev.data?.latest.length ? <Empty>–</Empty> : (
          <Table><thead><tr><Th>Score</Th><Th>Model</Th><Th>Route</Th><Th>Reasoning</Th><Th>Span</Th></tr></thead>
            <tbody>{ev.data.latest.map((r) => <tr key={r.span_id}><Td><Badge tone={r.score == null ? "muted" : r.score >= 4 ? "ok" : r.score <= 2 ? "err" : "muted"}>{r.score ?? "?"}</Badge></Td><Td className="font-mono">{r.model}</Td><Td className="font-mono text-muted">{r.route}</Td><Td className="text-xs">{r.reasoning}</Td><Td><Link href={`/p/${pid}/query?dataset=spans&span_id=${r.span_id}`} className="font-mono text-info hover:underline">{r.span_id.slice(0, 8)}…</Link></Td></tr>)}</tbody></Table>
        )}
      </Card>
    </div>
  );
}

// ---------------------------------------------------------------- agents

function Agents() {
  const pid = useProjectId();
  const [last] = useLastSeconds();
  const [user, setUser] = useState("");
  const q = useProjectQuery<{ runs: AgentRun[] }>(["agent-runs", last, user], `/agent-runs?last_seconds=${last}&user_id=${encodeURIComponent(user)}`);
  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <Input className="w-56" placeholder="user id" value={user} onChange={(e) => setUser(e.target.value)} />
        <span className="text-xs text-muted">Runs group gateway calls by <code>gen_ai.conversation.id</code> (header <code>x-galileo-conversation-id</code>, or the SDK&apos;s <code>agent.run()</code>).</span>
      </div>
      <ErrorBox error={q.error} />
      {q.data && (q.data.runs.length === 0 ? <Empty>No agent runs in this window.</Empty> : (
        <Table><thead><tr><Th>Last activity</Th><Th>Run</Th><Th>User</Th><Th>Route / model</Th><Th className="text-right">Turns</Th><Th className="text-right">Tool turns</Th><Th className="text-right">Tokens</Th><Th className="text-right">Cost</Th><Th className="text-right">Model time</Th><Th className="text-right">Errors</Th></tr></thead>
          <tbody>{q.data.runs.map((r) => <tr key={r.conversation_id} className="hover:bg-panel-2">
            <Td className="whitespace-nowrap text-muted">{fmtTime(r.last_seen)}</Td>
            <Td><Link href={`/p/${pid}/ai/agents/${r.conversation_id}`} className="font-mono text-info hover:underline">{r.conversation_id.slice(0, 12)}…</Link></Td>
            <Td className="font-mono">{r.user_id || <span className="text-muted">–</span>}</Td><Td className="font-mono text-muted">{r.route} · {r.model}</Td>
            <Td className="text-right">{fmtNum(r.turns, 0)}</Td><Td className="text-right">{fmtNum(r.tool_turns, 0)}</Td><Td className="text-right">{fmtNum(r.input_tokens + r.output_tokens, 0)}</Td><Td className="text-right">{fmtUsd(r.cost_usd)}</Td><Td className="text-right font-mono">{fmtMs(r.model_ms)}</Td>
            <Td className="text-right">{r.errors ? <Badge tone="err">{fmtNum(r.errors, 0)}</Badge> : "0"}</Td></tr>)}</tbody></Table>
      ))}
    </div>
  );
}

// ---------------------------------------------------------------- datasets + prompt CI

function Datasets() {
  const list = useProjectQuery<{ datasets: GoldenDataset[] }>(["datasets"], "/gateway/datasets");
  const [open, setOpen] = useState<string | null>(null);
  const detail = useProjectQuery<{ dataset: GoldenDataset; items: { id: string; input: { messages: { role: string; content: string }[] }; expected: string; rubric: string; source_span: string }[] }>(["dataset", open], `/gateway/datasets/${open}`, { enabled: !!open });
  const [name, setName] = useState(""); const [desc, setDesc] = useState("");
  const create = useProjectMutation<{ name: string; description: string }>((p, b) => post(`/api/projects/${p}/gateway/datasets`, b), [["datasets"]]);
  const remove = useProjectMutation<string>((p, id) => del(`/api/projects/${p}/gateway/datasets/${id}`), [["datasets"]]);
  const [newItem, setNewItem] = useState({ prompt: "", expected: "", rubric: "" });
  const addItem = useProjectMutation<{ id: string; items: unknown[] }>((p, b) => post(`/api/projects/${p}/gateway/datasets/${b.id}/items`, { items: b.items }), [["datasets"], ["dataset", open]]);
  const delItem = useProjectMutation<{ id: string; item: string }>((p, b) => del(`/api/projects/${p}/gateway/datasets/${b.id}/items/${b.item}`), [["datasets"], ["dataset", open]]);
  return (
    <div className="space-y-3">
      <p className="text-muted text-sm">Golden datasets: inputs with an expected answer or a rubric. Attach one to a prompt (Prompts → CI) so every new version is judged before it can be promoted. Add items here or from a call in Calls → drawer → &quot;add to dataset&quot;.</p>
      <ErrorBox error={list.error ?? create.error} />
      <div className="grid gap-3 md:grid-cols-2">
        <Card title="Datasets">
          {list.data && (list.data.datasets.length === 0 ? <Empty>No datasets yet.</Empty> : (
            <Table><thead><tr><Th>Name</Th><Th>Description</Th><Th className="text-right">Items</Th><Th></Th></tr></thead>
              <tbody>{list.data.datasets.map((d) => <tr key={d.id} className="hover:bg-panel-2 cursor-pointer" onClick={() => setOpen(d.id)}><Td className="font-medium">{d.name}</Td><Td className="text-muted">{d.description}</Td><Td className="text-right">{d.items}</Td><Td className="text-right"><button className="text-muted hover:text-err" onClick={(e) => { e.stopPropagation(); if (confirm("Delete dataset?")) remove.mutate(d.id); }}><Trash2 size={13} /></button></Td></tr>)}</tbody></Table>
          ))}
        </Card>
        <Card title="New dataset">
          <form className="space-y-2" onSubmit={async (e) => { e.preventDefault(); await create.mutateAsync({ name, description: desc }); setName(""); setDesc(""); }}>
            <div><Label>Name</Label><Input value={name} onChange={(e) => setName(e.target.value)} required placeholder="support-golden" /></div>
            <div><Label>Description</Label><Input value={desc} onChange={(e) => setDesc(e.target.value)} /></div>
            <Button type="submit" variant="primary" size="sm">Create</Button>
          </form>
        </Card>
      </div>
      <Drawer open={!!open} onClose={() => setOpen(null)} title={detail.data?.dataset.name ?? "Dataset"} width="w-[760px]">
        {detail.data && (
          <div className="space-y-3 text-sm">
            <form className="rounded border p-2 space-y-2" onSubmit={async (e) => { e.preventDefault(); await addItem.mutateAsync({ id: open!, items: [newItem] }); setNewItem({ prompt: "", expected: "", rubric: "" }); }}>
              <Label>Add item</Label>
              <Textarea rows={2} placeholder="user prompt" value={newItem.prompt} onChange={(e) => setNewItem({ ...newItem, prompt: e.target.value })} required />
              <div className="grid grid-cols-2 gap-2"><Textarea rows={2} placeholder="expected answer (optional)" value={newItem.expected} onChange={(e) => setNewItem({ ...newItem, expected: e.target.value })} /><Textarea rows={2} placeholder="rubric hint (optional)" value={newItem.rubric} onChange={(e) => setNewItem({ ...newItem, rubric: e.target.value })} /></div>
              <Button type="submit" size="sm">Add</Button>
            </form>
            {detail.data.items.length === 0 ? <Empty>No items.</Empty> : detail.data.items.map((it) => (
              <div key={it.id} className="rounded border p-2">
                <div className="flex items-start gap-2"><pre className="flex-1 whitespace-pre-wrap font-mono text-[11px]">{it.input.messages?.map((m) => `[${m.role}] ${m.content}`).join("\n")}</pre><button className="text-muted hover:text-err" onClick={() => delItem.mutate({ id: open!, item: it.id })}><Trash2 size={13} /></button></div>
                {it.expected && <div className="mt-1 text-[11px]"><span className="text-muted">expected: </span>{it.expected.slice(0, 300)}</div>}
                {it.rubric && <div className="text-[11px]"><span className="text-muted">rubric: </span>{it.rubric}</div>}
                {it.source_span && <div className="text-[10px] text-muted font-mono">from call {it.source_span.slice(0, 8)}…</div>}
              </div>
            ))}
          </div>
        )}
      </Drawer>
    </div>
  );
}

function AddToDataset({ spanId }: { spanId: string }) {
  const list = useProjectQuery<{ datasets: GoldenDataset[] }>(["datasets"], "/gateway/datasets");
  const [ds, setDs] = useState("");
  const [done, setDone] = useState(false);
  const add = useProjectMutation<{ id: string; span: string }>((p, b) => post(`/api/projects/${p}/gateway/datasets/${b.id}/items`, { span_ids: [b.span] }), [["datasets"]]);
  if (!list.data?.datasets.length) return null;
  return (
    <div className="flex items-center gap-2 text-xs">
      <span className="text-muted">Add to dataset</span>
      <Select value={ds} onChange={(e) => setDs(e.target.value)}><option value="">choose…</option>{list.data.datasets.map((d) => <option key={d.id} value={d.id}>{d.name}</option>)}</Select>
      <Button size="sm" disabled={!ds || done} onClick={async () => { await add.mutateAsync({ id: ds, span: spanId }); setDone(true); }}>{done ? "added" : "Add"}</Button>
    </div>
  );
}

function PromptCiCard({ prompt, versions, onChange }: { prompt: PromptSummary; versions: PromptVersion[]; onChange: () => void }) {
  const datasets = useProjectQuery<{ datasets: GoldenDataset[] }>(["datasets"], "/gateway/datasets");
  const routes = useProjectQuery<{ routes: Route[] }>(["routes"], "/gateway/routes");
  const [ci, setCi] = useState<PromptCi>(prompt.ci && "run_route" in (prompt.ci as object) ? (prompt.ci as PromptCi) : { dataset_id: null, run_route: "", judge_route: "", min_score: 3.5, required: false });
  const save = useProjectMutation<PromptCi>((p, b) => patch(`/api/projects/${p}/gateway/prompts/${prompt.id}/ci`, { ci: b }), [["prompt", prompt.id], ["prompts"]]);
  const run = useProjectMutation<number>((p, v) => post(`/api/projects/${p}/gateway/prompts/${prompt.id}/ci`, { version: v }), [["prompt", prompt.id]]);
  const promote = useProjectMutation<number>((p, v) => post(`/api/projects/${p}/gateway/prompts/${prompt.id}/promote`, { version: v }), [["prompt", prompt.id], ["prompts"]]);
  return (
    <div className="rounded border p-2 space-y-2 text-sm">
      <div className="text-[11px] uppercase tracking-wide text-muted">Prompt CI</div>
      <div className="grid grid-cols-4 gap-2">
        <div><Label>Dataset</Label><Select value={ci.dataset_id ?? ""} onChange={(e) => setCi({ ...ci, dataset_id: e.target.value || null })}><option value="">none</option>{datasets.data?.datasets.map((d) => <option key={d.id} value={d.id}>{d.name} ({d.items})</option>)}</Select></div>
        <div><Label>Run route</Label><Select value={ci.run_route} onChange={(e) => setCi({ ...ci, run_route: e.target.value })}><option value="">choose…</option>{routes.data?.routes.map((r) => <option key={r.id} value={r.alias}>{r.alias}</option>)}</Select></div>
        <div><Label>Judge route</Label><Select value={ci.judge_route} onChange={(e) => setCi({ ...ci, judge_route: e.target.value })}><option value="">choose…</option>{routes.data?.routes.map((r) => <option key={r.id} value={r.alias}>{r.alias}</option>)}</Select></div>
        <div><Label>Min score</Label><Input type="number" step="0.1" min={1} max={5} value={ci.min_score} onChange={(e) => setCi({ ...ci, min_score: Number(e.target.value) })} /></div>
      </div>
      <div className="flex items-center gap-3"><label className="flex items-center gap-2"><input type="checkbox" checked={ci.required} onChange={(e) => setCi({ ...ci, required: e.target.checked })} /> promotion requires a passing score</label><Button size="sm" onClick={async () => { await save.mutateAsync(ci); onChange(); }}>Save CI</Button><ErrorBox error={save.error ?? promote.error} /></div>
      <Table><thead><tr><Th>Version</Th><Th>CI</Th><Th className="text-right">Score</Th><Th></Th></tr></thead>
        <tbody>{versions.map((v) => <tr key={v.id}><Td className="font-mono">v{v.version}{prompt.promoted_version === v.version && <Badge tone="ok" className="ml-2">promoted</Badge>}</Td><Td><Badge tone={v.ci_status === "passed" ? "ok" : v.ci_status === "failed" || v.ci_status === "error" ? "err" : "muted"}>{v.ci_status ?? "none"}</Badge></Td><Td className="text-right tabular-nums">{v.ci_score?.toFixed(2) ?? "–"}</Td>
          <Td className="text-right whitespace-nowrap"><Button size="sm" variant="ghost" onClick={() => run.mutate(v.version)} disabled={!ci.dataset_id}>run CI</Button> <Button size="sm" variant="ghost" onClick={async () => { await promote.mutateAsync(v.version); onChange(); }} disabled={prompt.promoted_version === v.version}>promote</Button></Td></tr>)}</tbody></Table>
    </div>
  );
}
