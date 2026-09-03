"use client";

import { useState } from "react";
import clsx from "clsx";
import { useProjectId, useProjectQuery, useProjectMutation, useMe } from "@/lib/hooks";
import { post, del, patch } from "@/lib/api";
import { Button, Card, Table, Th, Td, Empty, Badge, Drawer, ErrorBox, Input, Label, Select, Textarea, Stat } from "@/components/ui";
import type { ApiKey, RedactionRule, Member, Invite, ApiToken, AuditRow, Channel, LogPipeline, LogProcessor, LogMatch, LogMetricRule, UsageRes, MaintenanceWindow, OncallSchedule, Trigger } from "@/lib/types";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { del as apiDel, get as apiGet, patch as apiPatch, post as apiPost, put as apiPut } from "@/lib/api";
import { ago, fmtTime, fmtNum, fmtMs, fmtDuration } from "@/lib/format";
import { API_BASE } from "@/lib/api";
import { Plus, Trash2, Copy, Pencil } from "lucide-react";
import { RecipientsEditor, type Recipient } from "@/components/recipients";
import { put } from "@/lib/api";
import { useEffect } from "react";

function IssueSettings() {
  const pid = useProjectId();
  const cur = useProjectQuery<{ issue_recipients: Recipient[]; issues_last_run: string | null }>(["issue-settings"], "/issue-settings");
  const [recipients, setRecipients] = useState<Recipient[]>([]);
  useEffect(() => { if (cur.data) setRecipients(cur.data.issue_recipients ?? []); }, [cur.data]);
  const save = useProjectMutation<Recipient[]>((p, r) => put(`/api/projects/${p}/issue-settings`, { issue_recipients: r }), [["issue-settings"]]);
  return (
    <div className="grid gap-3 md:grid-cols-2">
      <Card title="Issue notifications">
        <p className="text-muted text-sm mb-3">Where to send a message when a <b>new</b> issue appears or a resolved one <b>regresses</b>. Counts are refreshed every alert tick; last run {cur.data?.issues_last_run ? ago(cur.data.issues_last_run) : "never"}.</p>
        <RecipientsEditor value={recipients} onChange={setRecipients} />
        <ErrorBox error={save.error} />
        <Button className="mt-3" variant="primary" onClick={() => save.mutate(recipients)}>Save</Button>
      </Card>
      <Card title="Deploy markers from CI">
        <p className="text-muted text-sm">Post a deploy and every chart gets a marker, and issues record the version they were first seen on.</p>
        <pre className="mt-2 whitespace-pre-wrap rounded border bg-bg p-2 font-mono text-[11px]">{`curl -X POST ${API_BASE}/api/projects/${pid}/deploys \
  -H "Authorization: Bearer <session or key>" -H "content-type: application/json" \
  -d '{"version": "1.4.3", "service": "melea-api", "note": "release notes or commit"}'`}</pre>
        <p className="text-muted text-xs mt-2">Set <span className="font-mono">APP_VERSION</span> in the app to the same string so spans carry <span className="font-mono">service.version</span>.</p>
      </Card>
    </div>
  );
}

export default function SettingsPage() {
  const [tab, setTab] = useState<"keys" | "redaction" | "connect" | "project" | "issues" | "org" | "tokens" | "audit" | "notifications" | "health" | "logs">("keys");
  return (
    <div className="space-y-3">
      <div className="flex gap-1 border-b">
        {(["keys", "connect", "redaction", "logs", "notifications", "issues", "project", "org", "tokens", "audit", "health"] as const).map((t) => <button key={t} onClick={() => setTab(t)} className={clsx("px-3 py-1.5 text-sm capitalize border-b-2 -mb-px", tab === t ? "border-accent text-fg" : "border-transparent text-muted hover:text-fg")}>{t === "keys" ? "API keys" : t === "org" ? "Organization" : t === "tokens" ? "Personal tokens" : t === "health" ? "Galileo health" : t}</button>)}
      </div>
      {tab === "keys" && <Keys />}
      {tab === "connect" && <Connect />}
      {tab === "redaction" && <Redaction />}
      {tab === "project" && <ProjectSettings />}
      {tab === "issues" && <IssueSettings />}
      {tab === "org" && <OrgSettings />}
      {tab === "tokens" && <><Tokens /><div className="mt-3 grid gap-3 md:grid-cols-2"><TwoFactorCard /></div></>}
      {tab === "audit" && <Audit />}
      {tab === "notifications" && <><Notifications /><div className="grid gap-3 md:grid-cols-2"><MaintenanceCard /><OncallCard /></div></>}
      {tab === "health" && <GalileoHealth />}
      {tab === "logs" && <LogsSettings />}
    </div>
  );
}

function Keys() {
  const list = useProjectQuery<{ api_keys: ApiKey[] }>(["api-keys"], "/api-keys");
  const [creating, setCreating] = useState(false);
  const [form, setForm] = useState({ name: "", scopes: ["ingest", "gateway"] });
  const [created, setCreated] = useState<string | null>(null);
  const create = useProjectMutation<typeof form, { key: string }>((p, b) => post(`/api/projects/${p}/api-keys`, b), [["api-keys"]]);
  const revoke = useProjectMutation<string>((p, id) => del(`/api/projects/${p}/api-keys/${id}`), [["api-keys"]]);
  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between"><p className="text-muted text-sm">Keys authenticate OTLP ingest and gateway calls. The full key is shown once.</p><Button variant="primary" size="sm" onClick={() => { setCreated(null); setCreating(true); }}><Plus size={13} /> Key</Button></div>
      {list.data?.api_keys.length === 0 ? <Empty>No keys yet.</Empty> : (
        <Table><thead><tr><Th>Name</Th><Th>Prefix</Th><Th>Scopes</Th><Th>Created</Th><Th>Last used</Th><Th></Th></tr></thead>
          <tbody>{list.data?.api_keys.map((k) => <tr key={k.id} className={clsx(k.revoked_at && "opacity-50")}><Td className="font-medium">{k.name}</Td><Td className="font-mono">{k.key_prefix}…</Td><Td>{k.scopes.map((s) => <Badge key={s} className="mr-1">{s}</Badge>)}</Td><Td className="text-muted">{fmtTime(k.created_at)}</Td><Td className="text-muted">{ago(k.last_used_at)}</Td><Td className="text-right">{k.revoked_at ? <Badge tone="err">revoked</Badge> : <Button size="sm" variant="ghost" onClick={() => confirm("Revoke key? Apps using it will stop sending data.") && revoke.mutate(k.id)}><Trash2 size={12} /></Button>}</Td></tr>)}</tbody></Table>
      )}
      <Drawer open={creating} onClose={() => setCreating(false)} title="New API key">
        {created ? (
          <div className="space-y-3">
            <p className="text-sm">Copy this key now. It will not be shown again.</p>
            <div className="flex gap-2"><Input readOnly value={created} className="font-mono" /><Button onClick={() => navigator.clipboard.writeText(created)}><Copy size={13} /></Button></div>
            <Button variant="primary" onClick={() => setCreating(false)}>Done</Button>
          </div>
        ) : (
          <form className="space-y-3" onSubmit={async (e) => { e.preventDefault(); const r = await create.mutateAsync(form); setCreated(r.key); }}>
            <div><Label>Name</Label><Input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} required placeholder="erp-vet production" /></div>
            <div><Label>Scopes</Label>{["ingest", "gateway", "rum"].map((s) => <label key={s} className="mr-4 text-sm"><input type="checkbox" className="mr-1" checked={form.scopes.includes(s)} onChange={(e) => setForm({ ...form, scopes: e.target.checked ? [...form.scopes, s] : form.scopes.filter((x) => x !== s) })} />{s}</label>)}</div>
            <ErrorBox error={create.error} />
            <Button type="submit" variant="primary">Create</Button>
          </form>
        )}
      </Drawer>
    </div>
  );
}

function Connect() {
  const gw = API_BASE.replace(/\/$/, "");
  const otlp = gw.replace(":8080", ":4318");
  const snippets: { title: string; code: string }[] = [
    { title: "Browser (galileo-rum — page loads, fetches, JS errors, Web Vitals)", code: `<script src="${API_BASE}/rum.js" data-key="glk_... (rum scope)" data-service="my-web"\n        data-endpoint="${otlp}" data-propagate="https://api.example.com"></script>\n<!-- add 'traceparent' to the API's CORS allowed headers so fetches join the backend trace -->` },
    { title: "Any OpenTelemetry SDK (env vars)", code: `OTEL_EXPORTER_OTLP_ENDPOINT=${otlp}\nOTEL_EXPORTER_OTLP_HEADERS="Authorization=Bearer glk_..."\nOTEL_SERVICE_NAME=my-app` },
    { title: "Anthropic SDK through the gateway (Python)", code: `from anthropic import Anthropic\nclient = Anthropic(base_url="${gw}/gw", api_key="glk_...")\nclient.messages.create(model="assistant", max_tokens=1024, messages=[{"role": "user", "content": "hi"}])` },
    { title: "Node (@galileo/node)", code: `npm i @galileo/node\n# zero-code: node --import @galileo/node/register app.js\nGALILEO_ENDPOINT=${otlp}\nGALILEO_API_KEY=glk_...\nOTEL_SERVICE_NAME=my-node-app\n\n// or in code\nimport { init, galileoExpress } from "@galileo/node";\ninit({ endpoint: "${otlp}", apiKey: "glk_...", service: "my-node-app" });\napp.use(galileoExpress((req) => req.user && { id: req.user.id, email: req.user.email }));` },
    { title: "Python — FastAPI / Flask / any (galileo-python)", code: `pip install galileo-python[fastapi,sqlalchemy]\nGALILEO_ENDPOINT=${otlp}\nGALILEO_API_KEY=glk_...\nOTEL_SERVICE_NAME=my-service\n\nimport galileo; galileo.init()\nfrom galileo.fastapi import GalileoMiddleware\napp.add_middleware(GalileoMiddleware, identify=lambda scope: {"user_id": scope["state"].user.id})\ngalileo.sqlalchemy.instrument(engine)` },
    { title: "Rust (galileo crate)", code: `# Cargo.toml: galileo = { path = "…/galileo/sdk/rust/galileo" }\nGALILEO_ENDPOINT=${otlp}\nGALILEO_API_KEY=glk_...\nOTEL_SERVICE_NAME=my-rust-service\n\nlet _g = galileo::init(galileo::Config::from_env());\nlet app = Router::new().route(\"/x/{id}\", get(h)).layer(axum::middleware::from_fn(galileo::axum::middleware));\nlet row = galileo::sql!(\"SELECT * FROM t WHERE id = $1\", \"postgresql\", q.fetch_one(&pool));` },
    { title: "Android (sdk/android/galileo)", code: `Galileo.init(this, endpoint = "${otlp}", apiKey = "glk_...", service = "my-android-app")\nGalileo.setUser(id = user.id)\nOkHttpClient.Builder().addInterceptor(Galileo.okHttpInterceptor(propagateTo = listOf("https://api.example.com")))` },
    { title: "PHP / Laravel (galileo/php)", code: `composer require galileo/php\n# .env (the service provider is auto-discovered)\nGALILEO_ENDPOINT=${otlp}\nGALILEO_API_KEY=glk_...\nOTEL_SERVICE_NAME=my-laravel-app\nGALILEO_SQL_PARAMS=1` },
    { title: "OpenAI-compatible SDK through the gateway (Node)", code: `import OpenAI from "openai";\nconst client = new OpenAI({ baseURL: "${gw}/gw/v1", apiKey: "glk_..." });\nawait client.chat.completions.create({ model: "assistant", messages: [{ role: "user", content: "hi" }] });` },
    { title: "Join LLM calls to the request trace", code: `# send the active span's W3C context with each gateway call\nheaders = { "traceparent": "00-<trace_id>-<span_id>-01", "x-galileo-user-id": user.id, "x-galileo-tenant-id": tenant.id }` },
  ];
  return (
    <div className="grid gap-3 md:grid-cols-2">
      {snippets.map((s) => <Card key={s.title} title={s.title}><pre className="whitespace-pre-wrap rounded border bg-bg p-2 font-mono text-[11px]">{s.code}</pre></Card>)}
      <Card title="Endpoints"><div className="font-mono text-[12px] space-y-1"><div>OTLP/HTTP <span className="text-muted">{otlp}/v1/{"{traces,logs,metrics}"}</span></div><div>OTLP/gRPC <span className="text-muted">{otlp.replace("4318", "4317")}</span></div><div>Gateway <span className="text-muted">{gw}/gw/v1/messages · {gw}/gw/v1/chat/completions</span></div></div></Card>
    </div>
  );
}

function Redaction() {
  const list = useProjectQuery<{ rules: RedactionRule[]; defaults: RedactionRule["rule"][] }>(["redaction"], "/redaction-rules");
  const [form, setForm] = useState<{ type: "key" | "value"; pattern: string; action: "drop" | "mask" | "hash"; regex: string; replacement: string; description: string }>({ type: "key", pattern: "", action: "mask", regex: "", replacement: "[REDACTED]", description: "" });
  const [test, setTest] = useState({ text: "contact sam@example.com, card 4111 1111 1111 1111", out: "" });
  const pid = useProjectId();
  const create = useProjectMutation<unknown>((p, b) => post(`/api/projects/${p}/redaction-rules`, b), [["redaction"]]);
  const remove = useProjectMutation<string>((p, id) => del(`/api/projects/${p}/redaction-rules/${id}`), [["redaction"]]);
  const rule = () => (form.type === "key" ? { type: "key", pattern: form.pattern, action: form.action } : { type: "value", regex: form.regex, replacement: form.replacement });
  const desc = (r: RedactionRule["rule"]) => (r.type === "key" ? `key ${r.pattern} → ${r.action}` : `value /${r.regex}/ → ${r.replacement}`);
  return (
    <div className="grid gap-3 md:grid-cols-2">
      <div className="space-y-3">
        <Card title="Project rules">
          {list.data?.rules.length === 0 ? <Empty>Only the built-in defaults apply.</Empty> : list.data?.rules.map((r) => <div key={r.id} className="flex items-center justify-between border-b border-border/50 py-1.5 text-[12px]"><span className="font-mono">{desc(r.rule)}<span className="text-muted ml-2">{r.description}</span></span><button className="text-muted hover:text-err" onClick={() => remove.mutate(r.id)}><Trash2 size={12} /></button></div>)}
        </Card>
        <Card title="Built-in defaults (always on)">{list.data?.defaults.map((r, i) => <div key={i} className="font-mono text-[11px] text-muted py-0.5">{desc(r)}</div>)}</Card>
      </div>
      <div className="space-y-3">
        <Card title="Add rule">
          <form className="space-y-3" onSubmit={async (e) => { e.preventDefault(); await create.mutateAsync({ rule: rule(), description: form.description }); }}>
            <Select className="w-full" value={form.type} onChange={(e) => setForm({ ...form, type: e.target.value as "key" | "value" })}><option value="key">Match attribute key</option><option value="value">Match value with regex</option></Select>
            {form.type === "key" ? (
              <div className="grid grid-cols-2 gap-2"><div><Label>Key pattern (* wildcard)</Label><Input className="font-mono" value={form.pattern} onChange={(e) => setForm({ ...form, pattern: e.target.value })} placeholder="user.email" required /></div><div><Label>Action</Label><Select className="w-full" value={form.action} onChange={(e) => setForm({ ...form, action: e.target.value as "drop" | "mask" | "hash" })}><option value="mask">mask</option><option value="drop">drop</option><option value="hash">hash (keeps groupability)</option></Select></div></div>
            ) : (
              <div className="grid grid-cols-2 gap-2"><div><Label>Regex</Label><Input className="font-mono" value={form.regex} onChange={(e) => setForm({ ...form, regex: e.target.value })} placeholder="[\\w.+-]+@[\\w-]+\\.[\\w.]+" required /></div><div><Label>Replacement</Label><Input className="font-mono" value={form.replacement} onChange={(e) => setForm({ ...form, replacement: e.target.value })} /></div></div>
            )}
            <div><Label>Description</Label><Input value={form.description} onChange={(e) => setForm({ ...form, description: e.target.value })} /></div>
            <ErrorBox error={create.error} />
            <Button type="submit" variant="primary">Add rule</Button>
          </form>
        </Card>
        <Card title="Try it">
          <Textarea rows={2} value={test.text} onChange={(e) => setTest({ ...test, text: e.target.value })} />
          <Button size="sm" className="mt-2" onClick={async () => { const r = await post<{ text: string }>(`/api/projects/${pid}/redaction-rules/test`, { rules: [rule()], include_defaults: true, text: test.text }); setTest({ ...test, out: r.text }); }}>Run redaction</Button>
          {test.out && <pre className="mt-2 whitespace-pre-wrap rounded border bg-bg p-2 font-mono text-[11px]">{test.out}</pre>}
        </Card>
      </div>
    </div>
  );
}

const KIND_HELP: Record<Channel["kind"], string> = {
  webhook: "POST JSON to your endpoint", slack: "Slack incoming webhook (Block Kit message)", discord: "Discord channel webhook (embed)",
  telegram: "Bot token + chat id (BotFather → /newbot, then message the bot and read chat id from getUpdates)", email: "One or more addresses; needs SMTP on the server",
};

function Notifications() {
  const pid = useProjectId();
  const list = useProjectQuery<{ channels: Channel[]; email_enabled: boolean }>(["channels"], "/channels");
  const [edit, setEdit] = useState<Partial<Channel> | null>(null);
  const [testMsg, setTestMsg] = useState<Record<string, string>>({});
  const save = useProjectMutation<Partial<Channel>>((p, b) => (b.id ? put(`/api/projects/${p}/channels/${b.id}`, b) : post(`/api/projects/${p}/channels`, b)), [["channels"]]);
  const remove = useProjectMutation<string>((p, id) => del(`/api/projects/${p}/channels/${id}`), [["channels"]]);
  const digest = useProjectQuery<{ digest: { digest_channels: Recipient[]; digest_hour_utc: number; digest_last_sent: string | null } }>(["digest"], "/digest");
  const [dch, setDch] = useState<Recipient[] | null>(null);
  const [dhour, setDhour] = useState<number | null>(null);
  const saveDigest = useProjectMutation<{ digest_channels: Recipient[]; digest_hour_utc: number }>((p, b) => put(`/api/projects/${p}/digest`, b), [["digest"]]);
  const sendDigest = useProjectMutation<void, { sent: number }>((p) => post(`/api/projects/${p}/digest/send`, {}), []);
  const preview = useProjectQuery<{ text: string; date: string }>(["digest-preview"], "/digest/preview", { enabled: false });
  const cfgStr = (k: string) => String((edit?.config as Record<string, unknown> | undefined)?.[k] ?? "");
  const setCfg = (k: string, v: unknown) => setEdit({ ...edit, config: { ...(edit?.config ?? {}), [k]: v } });
  const channels = dch ?? digest.data?.digest.digest_channels ?? [];
  const hour = dhour ?? digest.data?.digest.digest_hour_utc ?? 8;
  return (
    <div className="grid gap-3 md:grid-cols-2">
      <Card title="Channels" actions={<Button size="sm" variant="primary" onClick={() => setEdit({ kind: "slack", name: "", config: {}, enabled: true })}><Plus size={13} /> Channel</Button>}>
        <p className="text-muted text-sm mb-3">Reusable destinations for triggers, SLO burn alerts, issues and the daily digest.{list.data && !list.data.email_enabled && <> E-mail channels need SMTP: set <span className="font-mono">GALILEO_SMTP_*</span> in <span className="font-mono">deploy/.env</span>.</>}</p>
        {list.data?.channels.length === 0 ? <Empty>No channels yet.</Empty> : (
          <Table><thead><tr><Th>Name</Th><Th>Kind</Th><Th>Destination</Th><Th></Th></tr></thead>
            <tbody>{list.data?.channels.map((c) => (
              <tr key={c.id} className={clsx(!c.enabled && "opacity-50")}>
                <Td className="font-medium">{c.name}</Td><Td><Badge>{c.kind}</Badge></Td>
                <Td className="font-mono text-[11px] text-muted max-w-[220px] truncate">{c.kind === "email" ? (c.config.to as string[] | undefined)?.join(", ") : c.kind === "telegram" ? `chat ${String(c.config.chat_id)}` : String(c.config.url ?? "")}</Td>
                <Td className="text-right whitespace-nowrap">
                  <Button size="sm" variant="ghost" onClick={async () => { try { await post(`/api/projects/${pid}/channels/${c.id}/test`); setTestMsg({ ...testMsg, [c.id]: "sent ✓" }); } catch (e) { setTestMsg({ ...testMsg, [c.id]: e instanceof Error ? e.message : "failed" }); } }}>Test</Button>
                  {testMsg[c.id] && <span className={clsx("mr-2 text-[11px]", testMsg[c.id].startsWith("sent") ? "text-ok" : "text-err")}>{testMsg[c.id]}</span>}
                  <Button size="sm" variant="ghost" onClick={() => setEdit({ ...c })}><Pencil size={12} /></Button>
                  <Button size="sm" variant="ghost" onClick={() => confirm(`Delete channel ${c.name}?`) && remove.mutate(c.id)}><Trash2 size={12} /></Button>
                </Td>
              </tr>
            ))}</tbody></Table>
        )}
      </Card>
      <Card title="Daily digest">
        <p className="text-muted text-sm mb-3">Yesterday&apos;s requests, errors, p95, LLM spend, open issues, SLO budgets and trigger events, once a day.</p>
        <Label>Send to</Label>
        <RecipientsEditor value={channels} onChange={setDch} />
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <Label>at (UTC hour)</Label>
          <Input type="number" min={0} max={23} className="w-20" value={hour} onChange={(e) => setDhour(Number(e.target.value))} />
          <Button variant="primary" size="sm" onClick={() => saveDigest.mutate({ digest_channels: channels, digest_hour_utc: hour })}>Save</Button>
          <Button size="sm" onClick={() => sendDigest.mutate()}>Send now</Button>
          <Button size="sm" variant="ghost" onClick={() => preview.refetch()}>Preview</Button>
          <span className="text-[11px] text-muted">last sent {digest.data?.digest.digest_last_sent ?? "never"}{sendDigest.data ? ` · sent to ${sendDigest.data.sent}` : ""}</span>
        </div>
        <ErrorBox error={saveDigest.error || sendDigest.error} />
        {preview.data && <pre className="mt-3 whitespace-pre-wrap rounded border bg-bg p-2 font-mono text-[11px]">{preview.data.text}</pre>}
      </Card>
      <Drawer open={!!edit} onClose={() => setEdit(null)} title={edit?.id ? "Edit channel" : "New channel"}>
        {edit && (
          <form className="space-y-3" onSubmit={async (e) => { e.preventDefault(); await save.mutateAsync(edit); setEdit(null); }}>
            <div><Label>Name</Label><Input required value={edit.name ?? ""} onChange={(e) => setEdit({ ...edit, name: e.target.value })} placeholder="#alerts" /></div>
            <div><Label>Kind</Label><Select className="w-full" value={edit.kind} onChange={(e) => setEdit({ ...edit, kind: e.target.value as Channel["kind"], config: {} })}>{(["slack", "discord", "telegram", "email", "webhook"] as const).map((k) => <option key={k} value={k}>{k}</option>)}</Select><p className="mt-1 text-[11px] text-muted">{KIND_HELP[edit.kind ?? "slack"]}</p></div>
            {(edit.kind === "slack" || edit.kind === "discord" || edit.kind === "webhook") && <div><Label>URL</Label><Input className="font-mono" required value={cfgStr("url")} onChange={(e) => setCfg("url", e.target.value)} placeholder="https://hooks.slack.com/services/…" /></div>}
            {edit.kind === "telegram" && <><div><Label>Bot token</Label><Input className="font-mono" required value={cfgStr("bot_token")} onChange={(e) => setCfg("bot_token", e.target.value)} /></div><div><Label>Chat id</Label><Input className="font-mono" required value={cfgStr("chat_id")} onChange={(e) => setCfg("chat_id", e.target.value)} /></div></>}
            {edit.kind === "email" && <div><Label>To (comma separated)</Label><Input required value={((edit.config?.to as string[] | undefined) ?? []).join(", ")} onChange={(e) => setCfg("to", e.target.value.split(",").map((x) => x.trim()).filter(Boolean))} placeholder="oncall@clinic.com, ti@clinic.com" /></div>}
            <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={edit.enabled ?? true} onChange={(e) => setEdit({ ...edit, enabled: e.target.checked })} /> Enabled</label>
            <ErrorBox error={save.error} />
            <Button type="submit" variant="primary">Save</Button>
          </form>
        )}
      </Drawer>
    </div>
  );
}

function AssistantCard({ orgId }: { orgId: string }) {
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["org-assistant", orgId], queryFn: () => apiGet<{ assistant: { enabled: boolean; project_id: string | null; route_alias: string; max_steps: number }; routes: string[] }>(`/api/orgs/${orgId}/assistant`), enabled: !!orgId });
  const me = useMe();
  const [f, setF] = useState<{ enabled: boolean; project_id: string | null; route_alias: string; max_steps: number } | null>(null);
  const v = f ?? q.data?.assistant ?? { enabled: false, project_id: null, route_alias: "", max_steps: 6 };
  const save = useMutation({ mutationFn: (b: typeof v) => apiPut(`/api/orgs/${orgId}/assistant`, b), onSuccess: () => { qc.invalidateQueries({ queryKey: ["org-assistant", orgId] }); setF(null); } });
  const [test, setTest] = useState<string>("");
  const pid = useProjectId();
  return (
    <Card title="Assistant (Ask Galileo)">
      <p className="text-muted text-sm mb-3">Answers questions, explains traces and writes root-cause notes using one of <b>your</b> gateway routes. Its calls are recorded in the chosen project like any other LLM call. A non-thinking model with good JSON output works best.</p>
      <form className="space-y-2" onSubmit={async (e) => { e.preventDefault(); await save.mutateAsync(v); }}>
        <div className="grid grid-cols-3 gap-2">
          <div><Label>Recording project</Label><Select className="w-full" value={v.project_id ?? ""} onChange={(e) => setF({ ...v, project_id: e.target.value || null, route_alias: "" })}><option value="">choose…</option>{me.data?.projects.filter((p) => p.org_id === orgId).map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}</Select></div>
          <div><Label>Gateway route</Label>{q.data?.routes.length && v.project_id === (q.data.assistant.project_id ?? v.project_id) ? <Select className="w-full" value={v.route_alias} onChange={(e) => setF({ ...v, route_alias: e.target.value })}><option value="">choose…</option>{q.data.routes.map((r) => <option key={r} value={r}>{r}</option>)}</Select> : <Input className="font-mono" value={v.route_alias} onChange={(e) => setF({ ...v, route_alias: e.target.value })} placeholder="galileo-assistant" />}</div>
          <div><Label>Tool steps per question</Label><Input type="number" min={1} max={12} value={v.max_steps} onChange={(e) => setF({ ...v, max_steps: Number(e.target.value) })} /></div>
        </div>
        <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={v.enabled} onChange={(e) => setF({ ...v, enabled: e.target.checked })} /> Enabled</label>
        <div className="flex items-center gap-2"><Button type="submit" variant="primary">Save</Button>
          <Button type="button" onClick={async () => { setTest("…"); try { const r = await apiPost<{ answer: string }>(`/api/projects/${pid}/assistant/chat`, { messages: [{ role: "user", content: "In one sentence: how many spans did this project receive in the last hour?" }], context: { page: "settings", last_seconds: 3600 } }); setTest(r.answer); } catch (e) { setTest(`error: ${(e as Error).message}`); } }}>Test</Button>
          {test && <span className="text-xs text-muted truncate max-w-[420px]" title={test}>{test}</span>}</div>
        <ErrorBox error={save.error} />
      </form>
    </Card>
  );
}

function OrgSettings() {
  const me = useMe();
  const pid = useProjectId();
  const orgId = me.data?.projects.find((p) => p.id === pid)?.org_id;
  const qc = useQueryClient();
  const members = useQuery({ queryKey: ["org-members", orgId], queryFn: () => apiGet<{ members: Member[]; my_role: string }>(`/api/orgs/${orgId}/members`), enabled: !!orgId });
  const invites = useQuery({ queryKey: ["org-invites", orgId], queryFn: () => apiGet<{ invites: Invite[] }>(`/api/orgs/${orgId}/invites`), enabled: !!orgId && ["owner", "admin"].includes(members.data?.my_role ?? "") });
  const [form, setForm] = useState({ email: "", role: "member" });
  const [link, setLink] = useState<string | null>(null);
  const invite = useMutation({ mutationFn: (b: typeof form) => apiPost<{ link: string }>(`/api/orgs/${orgId}/invites`, b), onSuccess: (r) => { setLink(r.link); qc.invalidateQueries({ queryKey: ["org-invites", orgId] }); } });
  const setRole = useMutation({ mutationFn: (b: { user_id: string; role: string }) => apiPatch(`/api/orgs/${orgId}/members/${b.user_id}`, { role: b.role }), onSuccess: () => qc.invalidateQueries({ queryKey: ["org-members", orgId] }) });
  const remove = useMutation({ mutationFn: (user_id: string) => apiDel(`/api/orgs/${orgId}/members/${user_id}`), onSuccess: () => qc.invalidateQueries({ queryKey: ["org-members", orgId] }) });
  const revoke = useMutation({ mutationFn: (id: string) => apiDel(`/api/orgs/${orgId}/invites/${id}`), onSuccess: () => qc.invalidateQueries({ queryKey: ["org-invites", orgId] }) });
  const canManage = ["owner", "admin"].includes(members.data?.my_role ?? "");
  const isOwner = members.data?.my_role === "owner";
  return (
    <div className="grid gap-3 md:grid-cols-2">
      {orgId && <AssistantCard orgId={orgId} />}
      <Card title="Members">
        <Table><thead><tr><Th>Member</Th><Th>Role</Th><Th></Th></tr></thead>
          <tbody>{members.data?.members.map((m) => <tr key={m.user_id}><Td><div className="font-medium">{m.name || m.email}</div><div className="text-[11px] text-muted">{m.email}</div></Td>
            <Td>{isOwner ? <Select value={m.role} onChange={(e) => setRole.mutate({ user_id: m.user_id, role: e.target.value })}>{["owner", "admin", "member", "viewer"].map((r) => <option key={r}>{r}</option>)}</Select> : <Badge>{m.role}</Badge>}</Td>
            <Td className="text-right">{canManage && <Button size="sm" variant="ghost" onClick={() => confirm(`Remove ${m.email} from the organization?`) && remove.mutate(m.user_id)}><Trash2 size={12} /></Button>}</Td></tr>)}</tbody></Table>
        <ErrorBox error={setRole.error || remove.error} />
        <p className="mt-2 text-[11px] text-muted">owner: everything · admin: manage projects, members, invites · member: configure and query · viewer: read only</p>
      </Card>
      {canManage && (
        <Card title="Invite someone">
          <form className="space-y-3" onSubmit={async (e) => { e.preventDefault(); await invite.mutateAsync(form); }}>
            <div className="flex gap-2"><Input type="email" required placeholder="colleague@clinic.com" value={form.email} onChange={(e) => setForm({ ...form, email: e.target.value })} /><Select value={form.role} onChange={(e) => setForm({ ...form, role: e.target.value })}><option value="admin">admin</option><option value="member">member</option><option value="viewer">viewer</option></Select><Button type="submit" variant="primary">Invite</Button></div>
            <ErrorBox error={invite.error} />
            {link && <div className="rounded border bg-bg p-2 text-xs"><div className="text-muted mb-1">Send this link (valid 7 days). E-mail delivery arrives in Phase 5.</div><div className="flex gap-2"><Input readOnly value={link} className="font-mono" /><Button onClick={() => navigator.clipboard.writeText(link)}><Copy size={13} /></Button></div></div>}
          </form>
          {!!invites.data?.invites.length && (
            <Table className="mt-3"><thead><tr><Th>Email</Th><Th>Role</Th><Th>Status</Th><Th></Th></tr></thead>
              <tbody>{invites.data.invites.map((i) => <tr key={i.id}><Td>{i.email}</Td><Td><Badge>{i.role}</Badge></Td><Td className="text-muted">{i.accepted_at ? `accepted ${ago(i.accepted_at)}` : new Date(i.expires_at) < new Date() ? "expired" : `pending · expires ${ago(i.expires_at).replace(" ago", "")}`}</Td><Td className="text-right">{!i.accepted_at && <Button size="sm" variant="ghost" onClick={() => revoke.mutate(i.id)}><Trash2 size={12} /></Button>}</Td></tr>)}</tbody></Table>
          )}
        </Card>
      )}
    </div>
  );
}

function Tokens() {
  const qc = useQueryClient();
  const list = useQuery({ queryKey: ["tokens"], queryFn: () => apiGet<{ tokens: ApiToken[] }>("/api/auth/tokens") });
  const [name, setName] = useState("");
  const [days, setDays] = useState("");
  const [created, setCreated] = useState<string | null>(null);
  const create = useMutation({ mutationFn: () => apiPost<{ token: string }>("/api/auth/tokens", { name, expires_days: days ? Number(days) : null }), onSuccess: (r) => { setCreated(r.token); setName(""); qc.invalidateQueries({ queryKey: ["tokens"] }); } });
  const revoke = useMutation({ mutationFn: (id: string) => apiDel(`/api/auth/tokens/${id}`), onSuccess: () => qc.invalidateQueries({ queryKey: ["tokens"] }) });
  return (
    <div className="grid gap-3 md:grid-cols-2">
      <Card title="Personal API tokens">
        <p className="text-muted text-sm mb-3">For scripts and CI calling the Galileo API as you: <span className="font-mono">Authorization: Bearer glt_…</span>. Same permissions as your account.</p>
        <form className="flex gap-2" onSubmit={(e) => { e.preventDefault(); create.mutate(); }}><Input required placeholder="token name (e.g. ci deploy markers)" value={name} onChange={(e) => setName(e.target.value)} /><Input className="w-28" type="number" placeholder="days" value={days} onChange={(e) => setDays(e.target.value)} /><Button type="submit" variant="primary">Create</Button></form>
        <ErrorBox error={create.error} />
        {created && <div className="mt-3 rounded border bg-bg p-2 text-xs"><div className="text-muted mb-1">Copy it now, it is shown once.</div><div className="flex gap-2"><Input readOnly value={created} className="font-mono" /><Button onClick={() => navigator.clipboard.writeText(created)}><Copy size={13} /></Button></div></div>}
      </Card>
      <Card title="Your tokens">
        {list.data?.tokens.length === 0 ? <Empty>No tokens.</Empty> : (
          <Table><thead><tr><Th>Name</Th><Th>Prefix</Th><Th>Last used</Th><Th>Expires</Th><Th></Th></tr></thead>
            <tbody>{list.data?.tokens.map((t) => <tr key={t.id} className={clsx(t.revoked_at && "opacity-50")}><Td>{t.name}</Td><Td className="font-mono">{t.token_prefix}…</Td><Td className="text-muted">{ago(t.last_used_at)}</Td><Td className="text-muted">{t.expires_at ? fmtTime(t.expires_at) : "never"}</Td><Td className="text-right">{t.revoked_at ? <Badge tone="err">revoked</Badge> : <Button size="sm" variant="ghost" onClick={() => revoke.mutate(t.id)}><Trash2 size={12} /></Button>}</Td></tr>)}</tbody></Table>
        )}
      </Card>
    </div>
  );
}

function Audit() {
  const rows = useProjectQuery<{ audit: AuditRow[] }>(["audit"], "/audit");
  return (
    <Card title="Audit log (this project)">
      {rows.data?.audit.length === 0 ? <Empty>No configuration changes recorded yet.</Empty> : (
        <Table className="max-h-[70vh]"><thead><tr><Th>When</Th><Th>Who</Th><Th>Action</Th><Th>Target</Th><Th>Details</Th></tr></thead>
          <tbody>{rows.data?.audit.map((a) => <tr key={a.id}><Td className="whitespace-nowrap text-muted">{fmtTime(a.at)}</Td><Td>{a.user_email || <span className="text-muted">system</span>}</Td><Td><Badge tone={a.action.endsWith(".delete") || a.action.endsWith(".revoke") ? "err" : a.action.endsWith(".create") ? "ok" : "muted"}>{a.action}</Badge></Td><Td className="font-mono text-[11px] text-muted">{a.target_type} {a.target_id.slice(0, 8)}</Td><Td className="font-mono text-[11px] text-muted max-w-[420px] truncate" title={JSON.stringify(a.details)}>{JSON.stringify(a.details)}</Td></tr>)}</tbody></Table>
      )}
    </Card>
  );
}

function Retention() {
  const cur = useProjectQuery<{ settings: { retention_spans_days: number | null; retention_logs_days: number | null; retention_metrics_days: number | null; retention_last_run: string | null }; global: { spans_days: number; logs_days: number; metrics_days: number } }>(["project-settings"], "/settings");
  const [f, setF] = useState<{ s: string; l: string; m: string } | null>(null);
  const save = useProjectMutation<{ s: string; l: string; m: string }>((p, b) => put(`/api/projects/${p}/settings`, { retention_spans_days: b.s ? Number(b.s) : null, retention_logs_days: b.l ? Number(b.l) : null, retention_metrics_days: b.m ? Number(b.m) : null }), [["project-settings"]]);
  const v = f ?? { s: String(cur.data?.settings.retention_spans_days ?? ""), l: String(cur.data?.settings.retention_logs_days ?? ""), m: String(cur.data?.settings.retention_metrics_days ?? "") };
  const g = cur.data?.global;
  return (
    <Card title="Retention">
      <p className="text-muted text-sm mb-3">Days to keep this project&apos;s data. Blank = the server default ({g ? `${g.spans_days} / ${g.logs_days} / ${g.metrics_days} days` : "…"}). Shorter values are applied hourly by deleting older rows.</p>
      <form className="grid grid-cols-3 gap-2" onSubmit={async (e) => { e.preventDefault(); await save.mutateAsync(v); }}>
        <div><Label>Spans</Label><Input type="number" min={1} max={3650} value={v.s} onChange={(e) => setF({ ...v, s: e.target.value })} placeholder={g ? String(g.spans_days) : ""} /></div>
        <div><Label>Logs</Label><Input type="number" min={1} max={3650} value={v.l} onChange={(e) => setF({ ...v, l: e.target.value })} placeholder={g ? String(g.logs_days) : ""} /></div>
        <div><Label>Metrics</Label><Input type="number" min={1} max={3650} value={v.m} onChange={(e) => setF({ ...v, m: e.target.value })} placeholder={g ? String(g.metrics_days) : ""} /></div>
        <div className="col-span-3 flex items-center gap-3"><Button type="submit" variant="primary">Save</Button><span className="text-[11px] text-muted">last run {ago(cur.data?.settings.retention_last_run ?? null)}</span></div>
      </form>
      <ErrorBox error={save.error} />
    </Card>
  );
}

function ProjectSettings() {
  const me = useMe();
  const pid = useProjectId();
  const project = me.data?.projects.find((p) => p.id === pid);
  const [name, setName] = useState(project?.name ?? "");
  const [org, setOrg] = useState("");
  const [newProject, setNewProject] = useState("");
  const rename = useProjectMutation<string>((p, n) => patch(`/api/projects/${p}`, { name: n }), []);
  const createProject = useProjectMutation<{ org_id: string; name: string }>((_p, b) => post(`/api/projects`, b), []);
  return (
    <div className="grid gap-3 md:grid-cols-2">
      <Card title="Project">
        <form className="space-y-3" onSubmit={async (e) => { e.preventDefault(); await rename.mutateAsync(name); me.refetch(); }}>
          <div><Label>Name</Label><Input value={name || project?.name || ""} onChange={(e) => setName(e.target.value)} /></div>
          <div className="text-[11px] text-muted font-mono">id {pid}</div>
          <ErrorBox error={rename.error} />
          <Button type="submit" variant="primary">Rename</Button>
        </form>
      </Card>
      <Retention />
      <SamplingCard />
      <QuotasCard />
      <UsageCard />
      <MembersCard />
      <ConfigAsCodeCard />
      <Card title="New project">
        <form className="space-y-3" onSubmit={async (e) => { e.preventDefault(); await createProject.mutateAsync({ org_id: org || me.data!.orgs[0].id, name: newProject }); me.refetch(); setNewProject(""); }}>
          <div><Label>Organization</Label><Select className="w-full" value={org} onChange={(e) => setOrg(e.target.value)}>{me.data?.orgs.map((o) => <option key={o.id} value={o.id}>{o.name}</option>)}</Select></div>
          <div><Label>Name</Label><Input value={newProject} onChange={(e) => setNewProject(e.target.value)} required /></div>
          <ErrorBox error={createProject.error} />
          <Button type="submit" variant="primary">Create project</Button>
        </form>
      </Card>
    </div>
  );
}

type ProjectMember = { user_id: string; email: string; name: string; org_role: string; project_role: string | null; effective: string };

function MembersCard() {
  const pid = useProjectId();
  const q = useProjectQuery<{ members: ProjectMember[]; my_role: string }>(["project-members"], "/members");
  const setRole = useProjectMutation<{ user_id: string; role: string | null }>((p, b) => b.role ? apiPut(`/api/projects/${p}/members/${b.user_id}`, { role: b.role }) : apiDel(`/api/projects/${p}/members/${b.user_id}`), [["project-members"]]);
  const admin = q.data?.my_role === "owner" || q.data?.my_role === "admin";
  return (
    <Card title="Members and roles">
      <p className="text-muted text-sm mb-2">Org owners and admins are admins everywhere. Others get <b>editor</b> (org member) or <b>viewer</b> (org viewer) unless a project role overrides it. Viewer reads, editor edits triggers/boards/routes, admin changes settings and members.</p>
      <Table>
        <thead><tr><Th>User</Th><Th>Org role</Th><Th>Project role</Th><Th>Effective</Th></tr></thead>
        <tbody>
          {q.data?.members.map((m) => (
            <tr key={m.user_id}>
              <Td>{m.name || m.email}<div className="text-[11px] text-muted">{m.email}</div></Td>
              <Td><Badge>{m.org_role}</Badge></Td>
              <Td>
                {admin && m.org_role !== "owner" && m.org_role !== "admin" ? (
                  <Select value={m.project_role ?? ""} onChange={(e) => setRole.mutate({ user_id: m.user_id, role: e.target.value || null })}>
                    <option value="">inherit</option><option value="viewer">viewer</option><option value="editor">editor</option><option value="admin">admin</option>
                  </Select>
                ) : <span className="text-muted text-xs">{m.project_role ?? "inherit"}</span>}
              </Td>
              <Td><Badge tone={m.effective === "viewer" ? "muted" : m.effective === "editor" ? "ok" : "accent"}>{m.effective}</Badge></Td>
            </tr>
          ))}
        </tbody>
      </Table>
      <ErrorBox error={setRole.error} />
      <p className="text-[11px] text-muted mt-2">project {pid}</p>
    </Card>
  );
}

function ConfigAsCodeCard() {
  const pid = useProjectId();
  const [bundle, setBundle] = useState("");
  const [result, setResult] = useState<{ dry_run?: boolean; applied?: boolean; changes: { section: string; name: string; action: string }[] } | null>(null);
  const run = useMutation({
    mutationFn: async (dry: boolean) => {
      const r = await fetch(`${API_BASE}/api/projects/${pid}/import?dry_run=${dry}`, { method: "POST", credentials: "include", headers: { "content-type": "application/yaml" }, body: bundle });
      const j = await r.json();
      if (!r.ok) throw new Error(j?.error?.message ?? r.statusText);
      return j;
    },
    onSuccess: (r) => setResult(r),
  });
  return (
    <Card title="Config as code">
      <p className="text-muted text-sm mb-2">Export triggers, SLOs, boards, gateway routes and prompts, redaction rules, the log pipeline, log metrics, channels and settings as YAML. Import applies a bundle by name (secrets stay <span className="font-mono">&lt;redacted&gt;</span> unless you fill them in). The <span className="font-mono">galileo</span> CLI does the same from a terminal.</p>
      <div className="flex gap-2 mb-2">
        <a className="rounded-md border px-3 py-1.5 text-sm hover:bg-panel-2" href={`${API_BASE}/api/projects/${pid}/export?format=yaml`} target="_blank" rel="noreferrer">Export YAML</a>
        <a className="rounded-md border px-3 py-1.5 text-sm hover:bg-panel-2" href={`${API_BASE}/api/projects/${pid}/export?format=json`} target="_blank" rel="noreferrer">Export JSON</a>
        <a className="rounded-md border px-3 py-1.5 text-sm hover:bg-panel-2" href={`${API_BASE}/api/docs`} target="_blank" rel="noreferrer">API reference</a>
      </div>
      <Textarea rows={8} className="w-full font-mono text-[11px]" placeholder={"triggers:\n  - name: request volume\n    ..."} value={bundle} onChange={(e) => setBundle(e.target.value)} />
      <div className="flex gap-2 mt-2">
        <Button onClick={() => run.mutate(true)} disabled={!bundle.trim() || run.isPending}>Dry run</Button>
        <Button variant="primary" onClick={() => run.mutate(false)} disabled={!bundle.trim() || run.isPending}>Apply</Button>
      </div>
      <ErrorBox error={run.error} />
      {result && (
        <div className="mt-2 text-xs">
          <div className="text-muted mb-1">{result.dry_run ? "Would apply" : "Applied"} {result.changes.length} change(s){result.changes.length === 0 ? " — everything already matches" : ""}</div>
          {result.changes.map((c, i) => <div key={i} className="font-mono">{c.action} {c.section}: {c.name}</div>)}
        </div>
      )}
    </Card>
  );
}

function TwoFactorCard() {
  const q = useQuery({ queryKey: ["2fa"], queryFn: () => apiGet<{ enabled: boolean }>("/api/auth/2fa") });
  const [setup, setSetup] = useState<{ secret: string; otpauth_url: string } | null>(null);
  const [code, setCode] = useState("");
  const qc = useQueryClient();
  const start = useMutation({ mutationFn: () => apiPost<{ secret: string; otpauth_url: string }>("/api/auth/2fa/setup"), onSuccess: (r) => setSetup(r) });
  const enable = useMutation({ mutationFn: () => apiPost("/api/auth/2fa/enable", { code }), onSuccess: () => { setSetup(null); setCode(""); qc.invalidateQueries({ queryKey: ["2fa"] }); } });
  const disable = useMutation({ mutationFn: () => apiPost("/api/auth/2fa/disable", { code }), onSuccess: () => { setCode(""); qc.invalidateQueries({ queryKey: ["2fa"] }); } });
  return (
    <Card title="Two-factor authentication">
      <p className="text-muted text-sm mb-2">Adds a TOTP code (Google Authenticator, 1Password, Aegis…) to password logins. SSO logins are governed by your identity provider.</p>
      {q.data?.enabled ? (
        <div className="space-y-2">
          <Badge tone="ok">enabled</Badge>
          <div className="flex gap-2 items-end"><div><Label>Code to disable</Label><Input value={code} onChange={(e) => setCode(e.target.value)} placeholder="123456" /></div><Button onClick={() => disable.mutate()}>Disable</Button></div>
          <ErrorBox error={disable.error} />
        </div>
      ) : setup ? (
        <div className="space-y-2">
          <p className="text-sm">Add this secret to your authenticator app, then enter the current code.</p>
          <pre className="rounded border bg-bg p-2 font-mono text-[11px] whitespace-pre-wrap break-all">{setup.secret}</pre>
          <p className="text-[11px] text-muted break-all">{setup.otpauth_url}</p>
          <div className="flex gap-2 items-end"><div><Label>Code</Label><Input value={code} onChange={(e) => setCode(e.target.value)} placeholder="123456" /></div><Button variant="primary" onClick={() => enable.mutate()}>Enable</Button></div>
          <ErrorBox error={enable.error} />
        </div>
      ) : (
        <Button onClick={() => start.mutate()}>Set up 2FA</Button>
      )}
    </Card>
  );
}

function SamplingCard() {
  const cur = useProjectQuery<{ settings: { sampling: { rate: number; keep_errors: boolean; slow_ms: number; keep_llm: boolean; decision_delay_secs: number } } }>(["project-settings"], "/settings");
  const [f, setF] = useState<{ rate: number; keep_errors: boolean; slow_ms: number; keep_llm: boolean; decision_delay_secs: number } | null>(null);
  const v = f ?? cur.data?.settings.sampling ?? { rate: 1, keep_errors: true, slow_ms: 2000, keep_llm: true, decision_delay_secs: 10 };
  const save = useProjectMutation<typeof v>((p, b) => put(`/api/projects/${p}/settings`, { sampling: b }), [["project-settings"]]);
  const stats = useProjectQuery<{ ingest: Record<string, number> }>(["system-stats"], "/../../system/stats", { refetchInterval: 10_000 });
  return (
    <Card title="Sampling (tail-based)">
      <p className="text-muted text-sm mb-3">Keep a share of ordinary traces; errors, slow requests and LLM/browser traces are always kept. Decided per whole trace after it goes quiet, so nothing is half-kept. 100% = no sampling.</p>
      <form className="space-y-2" onSubmit={async (e) => { e.preventDefault(); await save.mutateAsync(v); }}>
        <div className="flex items-center gap-3"><span className="w-40 text-xs text-muted">Keep {Math.round(v.rate * 100)}% of traces</span><input type="range" min={0} max={100} step={5} value={Math.round(v.rate * 100)} onChange={(e) => setF({ ...v, rate: Number(e.target.value) / 100 })} className="flex-1" /></div>
        <div className="grid grid-cols-2 gap-2 text-sm">
          <label className="flex items-center gap-2"><input type="checkbox" checked={v.keep_errors} onChange={(e) => setF({ ...v, keep_errors: e.target.checked })} /> always keep errors</label>
          <label className="flex items-center gap-2"><input type="checkbox" checked={v.keep_llm} onChange={(e) => setF({ ...v, keep_llm: e.target.checked })} /> always keep LLM &amp; browser traces</label>
          <div className="flex items-center gap-2"><Label>slow if root over</Label><Input type="number" className="w-24" value={v.slow_ms} onChange={(e) => setF({ ...v, slow_ms: Number(e.target.value) })} /><span className="text-muted text-xs">ms</span></div>
          <div className="flex items-center gap-2"><Label>decide after quiet</Label><Input type="number" className="w-20" value={v.decision_delay_secs} onChange={(e) => setF({ ...v, decision_delay_secs: Number(e.target.value) })} /><span className="text-muted text-xs">s</span></div>
        </div>
        <div className="flex items-center gap-3"><Button type="submit" variant="primary">Save</Button>
          {stats.data && <span className="text-[11px] text-muted">server-wide: kept {stats.data.ingest.sampled_kept ?? 0} · dropped {stats.data.ingest.sampled_dropped ?? 0} traces · {stats.data.ingest.sampled_buffered ?? 0} spans buffered</span>}</div>
      </form>
      <ErrorBox error={save.error} />
    </Card>
  );
}

function GalileoHealth() {
  const h = useQuery({ queryKey: ["system-detail"], queryFn: () => apiGet<Record<string, unknown>>("/api/system/detail"), refetchInterval: 10_000 });
  const d = h.data as { version?: string; uptime_secs?: number; ingest?: Record<string, number>; queue_capacity?: number; clickhouse?: { parts: { table: string; parts: number; rows: number; bytes: number; oldest_partition: string }[]; merges: number; disks: { name: string; free_space: number; total_space: number }[]; last_span_at: string }; postgres?: { size: number; idle: number }; gateway_5m?: { calls: number; errors: number; p95_ms: number }; evaluator_age_secs?: number | null } | undefined;
  if (h.error) return <ErrorBox error={h.error} />;
  if (!d) return null;
  const ing = d.ingest ?? {};
  const queueFill = d.queue_capacity ? (ing.queued_rows ?? 0) / d.queue_capacity : 0;
  const lastWriteAge = ing.last_write_at ? Math.max(0, Date.now() / 1000 - ing.last_write_at) : null;
  const maxParts = Math.max(0, ...(d.clickhouse?.parts ?? []).map((p) => Number(p.parts)));
  const tone = (bad: boolean, warn: boolean) => (bad ? "err" : warn ? "warn" : "ok") as "err" | "warn" | "ok";
  const gb = (b: number) => `${(b / 1e9).toFixed(1)} GB`;
  return (
    <div className="space-y-3">
      <div className="grid grid-cols-2 gap-3 md:grid-cols-6">
        <Stat label="Version" value={d.version ?? "?"} sub={`up ${fmtDuration(d.uptime_secs ?? 0)}`} />
        <Stat label="Ingest queue" value={`${Math.round(queueFill * 100)}%`} sub={`${fmtNum(ing.queued_rows ?? 0, 0)} / ${fmtNum(d.queue_capacity ?? 0, 0)} rows`} tone={tone(queueFill > 0.95, queueFill > 0.8)} />
        <Stat label="Last write" value={lastWriteAge == null ? "never" : `${Math.round(lastWriteAge)}s ago`} tone={lastWriteAge == null ? undefined : tone(lastWriteAge > 120, lastWriteAge > 30)} />
        <Stat label="Rows written" value={fmtNum(ing.rows_written ?? 0)} sub={`${fmtNum(ing.rows_dropped ?? 0, 0)} dropped · ${fmtNum(ing.rejected_backpressure ?? 0, 0)} backpressure`} tone={tone((ing.write_errors ?? 0) > 0, (ing.rejected_backpressure ?? 0) > 0)} />
        <Stat label="Sampling" value={`${fmtNum(ing.sampled_kept ?? 0, 0)} kept`} sub={`${fmtNum(ing.sampled_dropped ?? 0, 0)} dropped · ${fmtNum(ing.sampled_buffered ?? 0, 0)} buffered`} />
        <Stat label="Evaluator" value={d.evaluator_age_secs == null ? "–" : `${Math.round(d.evaluator_age_secs)}s ago`} tone={d.evaluator_age_secs == null ? undefined : tone(d.evaluator_age_secs > 600, d.evaluator_age_secs > 120)} />
      </div>
      <div className="grid gap-3 md:grid-cols-2">
        <Card title={`ClickHouse · ${d.clickhouse?.merges ?? 0} merges in flight`}>
          <Table><thead><tr><Th>Table</Th><Th className="text-right">Parts</Th><Th className="text-right">Rows</Th><Th className="text-right">On disk</Th><Th>Oldest partition</Th></tr></thead>
            <tbody>{(d.clickhouse?.parts ?? []).map((p) => <tr key={p.table}><Td className="font-mono">{p.table}</Td><Td className="text-right"><Badge tone={tone(Number(p.parts) > 300, Number(p.parts) > 150)}>{p.parts}</Badge></Td><Td className="text-right">{fmtNum(Number(p.rows), 0)}</Td><Td className="text-right">{gb(Number(p.bytes))}</Td><Td className="font-mono text-muted">{p.oldest_partition}</Td></tr>)}</tbody></Table>
          <div className="mt-2 text-[11px] text-muted">disks: {(d.clickhouse?.disks ?? []).map((x) => `${x.name} ${gb(Number(x.total_space) - Number(x.free_space))} used of ${gb(Number(x.total_space))}`).join(" · ")} · last span {d.clickhouse?.last_span_at ? ago(d.clickhouse.last_span_at) : "–"} · max parts {maxParts}</div>
        </Card>
        <Card title="Postgres · gateway">
          <div className="grid grid-cols-2 gap-3">
            <Stat label="PG pool" value={`${d.postgres?.idle ?? 0} idle / ${d.postgres?.size ?? 0}`} />
            <Stat label="Gateway p95 (5m)" value={fmtMs(Number(d.gateway_5m?.p95_ms ?? 0))} sub={`${fmtNum(Number(d.gateway_5m?.calls ?? 0), 0)} calls · ${fmtNum(Number(d.gateway_5m?.errors ?? 0), 0)} errors`} tone={Number(d.gateway_5m?.calls) > 0 && Number(d.gateway_5m?.errors) / Number(d.gateway_5m?.calls) > 0.1 ? "warn" : undefined} />
          </div>
          <p className="mt-3 text-[11px] text-muted">Every 30 s these numbers are also written as <code>galileo.health.*</code> gauges into the Default project, so you can put triggers on Galileo itself.</p>
        </Card>
      </div>
    </div>
  );
}

function MatchEditor({ value, onChange, optional }: { value: LogMatch | null | undefined; onChange: (m: LogMatch | null) => void; optional?: boolean }) {
  const m = value ?? null;
  if (!m) return optional ? <Button size="sm" variant="ghost" onClick={() => onChange({ field: "body", op: "contains", value: "" })}>+ only when…</Button> : null;
  return (
    <div className="flex items-center gap-1 text-xs">
      <span className="text-muted">when</span>
      <Input className="w-28 font-mono" value={m.field} onChange={(e) => onChange({ ...m, field: e.target.value })} placeholder="body" />
      <Select value={m.op} onChange={(e) => onChange({ ...m, op: e.target.value })}>{["contains", "eq", "ne", "starts_with", "regex", "exists", "not_exists"].map((o) => <option key={o} value={o}>{o}</option>)}</Select>
      {!["exists", "not_exists"].includes(m.op) && <Input className="w-40 font-mono" value={m.value} onChange={(e) => onChange({ ...m, value: e.target.value })} />}
      {optional && <button className="text-muted hover:text-err" onClick={() => onChange(null)}><Trash2 size={12} /></button>}
    </div>
  );
}

function LogsSettings() {
  const pid = useProjectId();
  const cur = useProjectQuery<{ pipeline: LogPipeline }>(["log-pipeline"], "/log-pipeline");
  const metrics = useProjectQuery<{ metrics: { id: string; name: string; rule: LogMetricRule }[] }>(["log-metrics"], "/log-metrics");
  const [p, setP] = useState<LogPipeline | null>(null);
  const pipeline = p ?? cur.data?.pipeline ?? { enabled: true, processors: [] };
  const save = useProjectMutation<LogPipeline>((pr, b) => put(`/api/projects/${pr}/log-pipeline`, b), [["log-pipeline"]]);
  const [preview, setPreview] = useState<{ records: { timestamp: string; kept: boolean; before: { severity: string; body: string; attrs: Record<string, string> }; after: { severity: string; body: string; attrs: Record<string, string> }; metrics: { name: string; value: number }[] }[] } | null>(null);
  const [nm, setNm] = useState<LogMetricRule>({ name: "", match: { field: "body", op: "contains", value: "" }, value_from: "", unit: "1", enabled: true });
  const addMetric = useProjectMutation<LogMetricRule>((pr, b) => post(`/api/projects/${pr}/log-metrics`, { ...b, value_from: b.value_from || null }), [["log-metrics"]]);
  const delMetric = useProjectMutation<string>((pr, id) => del(`/api/projects/${pr}/log-metrics/${id}`), [["log-metrics"]]);
  const set = (i: number, np: LogProcessor) => setP({ ...pipeline, processors: pipeline.processors.map((x, j) => (j === i ? np : x)) });
  const move = (i: number, d: number) => { const a = [...pipeline.processors]; const j = i + d; if (j < 0 || j >= a.length) return; [a[i], a[j]] = [a[j], a[i]]; setP({ ...pipeline, processors: a }); };
  const add = (type: LogProcessor["type"]) => {
    const np: LogProcessor = type === "json_parse" ? { type, keep_body: false } : type === "regex_extract" ? { type, pattern: "user=(?P<user>\\w+)", field: "body" } : type === "rename" ? { type, from: "", to: "" } : type === "drop" ? { type, when: { field: "body", op: "contains", value: "healthz" } } : type === "severity_map" ? { type, rules: [{ pattern: "(?i)error|failed", severity: "error" }] } : { type, key: "", value: "" };
    setP({ ...pipeline, processors: [...pipeline.processors, np] });
  };
  return (
    <div className="space-y-3">
      <Card title="Log pipeline" actions={<label className="flex items-center gap-2 text-xs"><input type="checkbox" checked={pipeline.enabled} onChange={(e) => setP({ ...pipeline, enabled: e.target.checked })} /> enabled</label>}>
        <p className="text-muted text-sm mb-2">Processors run in order on every incoming log: parse JSON bodies into attributes, extract named groups with a regex, rename keys, drop noise, map severities, add fields. Preview shows recent logs before and after.</p>
        {pipeline.processors.length === 0 && <Empty>No processors yet.</Empty>}
        <ol className="space-y-2">
          {pipeline.processors.map((pr, i) => (
            <li key={i} className="rounded border p-2 space-y-1.5">
              <div className="flex items-center gap-2 text-sm"><Badge tone="accent">{i + 1}</Badge><b className="font-mono">{pr.type}</b>
                <span className="ml-auto flex gap-1 text-muted"><button onClick={() => move(i, -1)} className="hover:text-fg">↑</button><button onClick={() => move(i, 1)} className="hover:text-fg">↓</button><button onClick={() => setP({ ...pipeline, processors: pipeline.processors.filter((_, j) => j !== i) })} className="hover:text-err"><Trash2 size={13} /></button></span></div>
              {pr.type === "json_parse" && <label className="flex items-center gap-2 text-xs"><input type="checkbox" checked={!!pr.keep_body} onChange={(e) => set(i, { ...pr, keep_body: e.target.checked })} /> keep the raw JSON as the body (otherwise <code>message</code>/<code>msg</code> becomes the body)</label>}
              {pr.type === "regex_extract" && <div className="flex items-center gap-2 text-xs"><span className="text-muted">field</span><Input className="w-28 font-mono" value={pr.field ?? "body"} onChange={(e) => set(i, { ...pr, field: e.target.value })} /><span className="text-muted">pattern (named groups → attributes)</span><Input className="flex-1 font-mono" value={pr.pattern} onChange={(e) => set(i, { ...pr, pattern: e.target.value })} /></div>}
              {pr.type === "rename" && <div className="flex items-center gap-2 text-xs"><Input className="w-40 font-mono" value={pr.from} onChange={(e) => set(i, { ...pr, from: e.target.value })} placeholder="from" /><span>→</span><Input className="w-40 font-mono" value={pr.to} onChange={(e) => set(i, { ...pr, to: e.target.value })} placeholder="to" /></div>}
              {pr.type === "add_field" && <div className="flex items-center gap-2 text-xs"><Input className="w-40 font-mono" value={pr.key} onChange={(e) => set(i, { ...pr, key: e.target.value })} placeholder="key" /><span>=</span><Input className="w-40 font-mono" value={pr.value} onChange={(e) => set(i, { ...pr, value: e.target.value })} placeholder="value" /></div>}
              {pr.type === "severity_map" && <div className="space-y-1 text-xs">{pr.rules.map((r, k) => <div key={k} className="flex items-center gap-2"><Input className="flex-1 font-mono" value={r.pattern} onChange={(e) => set(i, { ...pr, rules: pr.rules.map((x, l) => (l === k ? { ...x, pattern: e.target.value } : x)) })} /><span>→</span><Select value={r.severity} onChange={(e) => set(i, { ...pr, rules: pr.rules.map((x, l) => (l === k ? { ...x, severity: e.target.value } : x)) })}>{["trace", "debug", "info", "warn", "error", "fatal"].map((sv) => <option key={sv} value={sv}>{sv}</option>)}</Select><button className="text-muted hover:text-err" onClick={() => set(i, { ...pr, rules: pr.rules.filter((_, l) => l !== k) })}><Trash2 size={12} /></button></div>)}<Button size="sm" variant="ghost" onClick={() => set(i, { ...pr, rules: [...pr.rules, { pattern: "", severity: "warn" }] })}>+ rule</Button></div>}
              {pr.type === "drop" ? <MatchEditor value={pr.when} onChange={(m) => set(i, { ...pr, when: m ?? { field: "body", op: "contains", value: "" } })} /> : <MatchEditor value={pr.when} onChange={(m) => set(i, { ...pr, when: m })} optional />}
            </li>
          ))}
        </ol>
        <div className="mt-2 flex flex-wrap gap-1">{(["json_parse", "regex_extract", "rename", "drop", "severity_map", "add_field"] as const).map((t) => <Button key={t} size="sm" onClick={() => add(t)}><Plus size={12} /> {t}</Button>)}</div>
        <div className="mt-3 flex items-center gap-2"><Button variant="primary" onClick={() => save.mutate(pipeline)}>Save pipeline</Button>
          <Button onClick={async () => setPreview(await post(`/api/projects/${pid}/log-pipeline/preview`, { pipeline, sample: 15, metrics: (metrics.data?.metrics ?? []).map((m) => m.rule) }))}>Preview on recent logs</Button><ErrorBox error={save.error} /></div>
        {preview && (
          <div className="mt-3 space-y-1 max-h-96 overflow-auto scroll-thin">
            {preview.records.map((r, i) => (
              <div key={i} className={clsx("rounded border p-2 text-[11px]", !r.kept && "opacity-50 border-err/40")}>
                <div className="flex gap-2 text-muted"><span>{fmtTime(r.timestamp)}</span>{!r.kept && <Badge tone="err">dropped</Badge>}{r.before.severity !== r.after.severity && <Badge tone="warn">{r.before.severity} → {r.after.severity}</Badge>}{r.metrics.map((m) => <Badge key={m.name} tone="ok">log.{m.name} = {m.value}</Badge>)}</div>
                <div className="grid grid-cols-2 gap-2 mt-1"><pre className="whitespace-pre-wrap font-mono">{r.before.body.slice(0, 300)}</pre><pre className="whitespace-pre-wrap font-mono">{r.after.body.slice(0, 300)}</pre></div>
                {Object.keys(r.after.attrs).filter((k) => !(k in r.before.attrs) || r.before.attrs[k] !== r.after.attrs[k]).length > 0 && <div className="mt-1 text-ok font-mono">+ {Object.entries(r.after.attrs).filter(([k, v]) => r.before.attrs[k] !== v).map(([k, v]) => `${k}=${v}`).join("  ")}</div>}
              </div>
            ))}
          </div>
        )}
      </Card>
      <Card title="Log-based metrics">
        <p className="text-muted text-sm mb-2">Turn matching logs into metric points named <code>log.&lt;name&gt;</code> (count per record, or the numeric value of an attribute). They chart, trigger and board like any metric.</p>
        {metrics.data && metrics.data.metrics.length > 0 && <Table><thead><tr><Th>Metric</Th><Th>Match</Th><Th>Value</Th><Th></Th></tr></thead><tbody>{metrics.data.metrics.map((m) => <tr key={m.id}><Td className="font-mono">log.{m.name}</Td><Td className="font-mono text-muted">{m.rule.match.field} {m.rule.match.op} {m.rule.match.value}</Td><Td className="text-muted">{m.rule.value_from ? `attr ${m.rule.value_from}` : "count"}</Td><Td className="text-right"><button className="text-muted hover:text-err" onClick={() => delMetric.mutate(m.id)}><Trash2 size={13} /></button></Td></tr>)}</tbody></Table>}
        <div className="mt-2 flex flex-wrap items-end gap-2 text-xs">
          <div><Label>name</Label><Input className="w-40 font-mono" value={nm.name} onChange={(e) => setNm({ ...nm, name: e.target.value })} placeholder="login_failures" /></div>
          <MatchEditor value={nm.match} onChange={(m) => setNm({ ...nm, match: m ?? nm.match })} />
          <div><Label>value from attribute (blank = count)</Label><Input className="w-40 font-mono" value={nm.value_from ?? ""} onChange={(e) => setNm({ ...nm, value_from: e.target.value })} /></div>
          <div><Label>unit</Label><Input className="w-16" value={nm.unit ?? ""} onChange={(e) => setNm({ ...nm, unit: e.target.value })} /></div>
          <Button size="sm" variant="primary" onClick={async () => { await addMetric.mutateAsync(nm); setNm({ ...nm, name: "" }); }} disabled={!nm.name}>Add</Button>
        </div>
        <ErrorBox error={addMetric.error} />
      </Card>
    </div>
  );
}

function QuotasCard() {
  const u = useProjectQuery<UsageRes>(["usage", 7], "/usage?last_seconds=604800");
  const [f, setF] = useState<{ spans_per_day: string; logs_per_day: string; metrics_per_day: string; mode: string } | null>(null);
  const q = u.data?.quotas ?? {};
  const v = f ?? { spans_per_day: String(q.spans_per_day ?? ""), logs_per_day: String(q.logs_per_day ?? ""), metrics_per_day: String(q.metrics_per_day ?? ""), mode: q.mode ?? "warn" };
  const save = useProjectMutation<typeof v>((p, b) => put(`/api/projects/${p}/quotas`, { spans_per_day: b.spans_per_day ? Number(b.spans_per_day) : null, logs_per_day: b.logs_per_day ? Number(b.logs_per_day) : null, metrics_per_day: b.metrics_per_day ? Number(b.metrics_per_day) : null, mode: b.mode }), [["usage", 7]]);
  return (
    <Card title="Ingest quotas">
      <p className="text-muted text-sm mb-2">Rows per day. <b>warn</b> notifies the issue recipients once a day per signal; <b>hard</b> answers 429 to the SDK once exceeded (counted in memory since the last restart, so treat it as a circuit breaker).</p>
      <form className="grid grid-cols-4 gap-2 text-sm" onSubmit={async (e) => { e.preventDefault(); await save.mutateAsync(v); }}>
        <div><Label>Spans / day</Label><Input type="number" value={v.spans_per_day} onChange={(e) => setF({ ...v, spans_per_day: e.target.value })} placeholder="unlimited" /></div>
        <div><Label>Logs / day</Label><Input type="number" value={v.logs_per_day} onChange={(e) => setF({ ...v, logs_per_day: e.target.value })} placeholder="unlimited" /></div>
        <div><Label>Metrics / day</Label><Input type="number" value={v.metrics_per_day} onChange={(e) => setF({ ...v, metrics_per_day: e.target.value })} placeholder="unlimited" /></div>
        <div><Label>Mode</Label><Select className="w-full" value={v.mode} onChange={(e) => setF({ ...v, mode: e.target.value })}><option value="warn">warn</option><option value="hard">hard</option></Select></div>
        <div className="col-span-4 flex items-center gap-3"><Button type="submit" variant="primary" size="sm">Save</Button>{u.data && <span className="text-[11px] text-muted">today: {fmtNum(u.data.today.spans, 0)} spans · {fmtNum(u.data.today.logs, 0)} logs · {fmtNum(u.data.today.metrics, 0)} metric points · rejected {fmtNum(u.data.ingest.quota_rejected ?? 0, 0)}</span>}</div>
      </form>
      <ErrorBox error={save.error} />
    </Card>
  );
}

function UsageCard() {
  const u = useProjectQuery<UsageRes>(["usage", 7], "/usage?last_seconds=604800");
  if (!u.data) return null;
  const gb = (b: number) => b >= 1e9 ? `${(b / 1e9).toFixed(2)} GB` : `${(b / 1e6).toFixed(1)} MB`;
  return (
    <Card title="Usage (7 days)">
      <div className="grid grid-cols-3 gap-2 mb-2">{(["spans", "logs", "metrics"] as const).map((k) => <Stat key={k} label={k} value={fmtNum(u.data!.totals[k]?.rows ?? 0, 0)} sub={`≈ ${gb(u.data!.totals[k]?.est_bytes ?? 0)} on disk`} />)}</div>
      <div className="text-[11px] uppercase tracking-wide text-muted mb-1">Highest-cardinality span attributes</div>
      <Table><thead><tr><Th>Attribute</Th><Th className="text-right">Distinct values</Th><Th className="text-right">Rows</Th></tr></thead>
        <tbody>{u.data.cardinality.slice(0, 10).map((c) => <tr key={c.k}><Td className="font-mono">{c.k}</Td><Td className="text-right">{fmtNum(Number(c.distinct_values), 0)}</Td><Td className="text-right">{fmtNum(Number(c.rows), 0)}</Td></tr>)}</tbody></Table>
    </Card>
  );
}

function MaintenanceCard() {
  const list = useProjectQuery<{ windows: MaintenanceWindow[] }>(["maintenance"], "/maintenance-windows");
  const triggers = useProjectQuery<{ triggers: Trigger[] }>(["triggers"], "/triggers");
  const local = (d: Date) => new Date(d.getTime() - d.getTimezoneOffset() * 60000).toISOString().slice(0, 16);
  const [f, setF] = useState({ name: "", starts_at: local(new Date()), ends_at: local(new Date(Date.now() + 3600_000)), trigger_ids: [] as string[] });
  const create = useProjectMutation<typeof f>((p, b) => post(`/api/projects/${p}/maintenance-windows`, { ...b, starts_at: new Date(b.starts_at).toISOString(), ends_at: new Date(b.ends_at).toISOString() }), [["maintenance"]]);
  const remove = useProjectMutation<string>((p, id) => del(`/api/projects/${p}/maintenance-windows/${id}`), [["maintenance"]]);
  return (
    <Card title="Maintenance windows">
      <p className="text-muted text-sm mb-2">Triggers keep evaluating inside a window but do not notify. Leave the trigger list empty to cover every trigger of the project.</p>
      {list.data && list.data.windows.length > 0 && <ul className="mb-2 space-y-1 text-xs">{list.data.windows.map((w) => <li key={w.id} className="flex items-center gap-2"><span className="font-medium">{w.name}</span><span className="text-muted">{fmtTime(w.starts_at)} → {fmtTime(w.ends_at)}</span>{new Date(w.starts_at) <= new Date() && new Date(w.ends_at) > new Date() && <Badge tone="warn">active</Badge>}<span className="text-muted">{w.trigger_ids.length ? `${w.trigger_ids.length} triggers` : "all triggers"}</span><button className="ml-auto text-muted hover:text-err" onClick={() => remove.mutate(w.id)}><Trash2 size={12} /></button></li>)}</ul>}
      <form className="space-y-2 text-sm" onSubmit={async (e) => { e.preventDefault(); await create.mutateAsync(f); setF({ ...f, name: "" }); }}>
        <div className="grid grid-cols-3 gap-2"><div><Label>Name</Label><Input value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} required placeholder="DB upgrade" /></div><div><Label>Starts</Label><Input type="datetime-local" value={f.starts_at} onChange={(e) => setF({ ...f, starts_at: e.target.value })} /></div><div><Label>Ends</Label><Input type="datetime-local" value={f.ends_at} onChange={(e) => setF({ ...f, ends_at: e.target.value })} /></div></div>
        <div className="flex flex-wrap gap-2 text-xs">{triggers.data?.triggers.map((t) => <label key={t.id} className="flex items-center gap-1"><input type="checkbox" checked={f.trigger_ids.includes(t.id)} onChange={(e) => setF({ ...f, trigger_ids: e.target.checked ? [...f.trigger_ids, t.id] : f.trigger_ids.filter((x) => x !== t.id) })} /> {t.name}</label>)}</div>
        <Button type="submit" variant="primary" size="sm">Add window</Button><ErrorBox error={create.error} />
      </form>
    </Card>
  );
}

function OncallCard() {
  const pid = useProjectId();
  const me = useMe();
  const orgId = me.data?.projects.find((p) => p.id === pid)?.org_id;
  const members = useQuery({ queryKey: ["org-members", orgId], queryFn: () => apiGet<{ members: Member[] }>(`/api/orgs/${orgId}/members`), enabled: !!orgId });
  const list = useProjectQuery<{ schedules: OncallSchedule[] }>(["oncall"], "/oncall");
  const channels = useProjectQuery<{ channels: Channel[] }>(["channels"], "/channels");
  const [f, setF] = useState({ name: "", members: [] as string[], rotation_days: 7, starts_on: new Date().toISOString().slice(0, 10), escalation: [] as { after_secs: number; channel_id: string }[] });
  const create = useProjectMutation<typeof f>((p, b) => post(`/api/projects/${p}/oncall`, b), [["oncall"]]);
  const remove = useProjectMutation<string>((p, id) => del(`/api/projects/${p}/oncall/${id}`), [["oncall"]]);
  return (
    <Card title="On-call">
      <p className="text-muted text-sm mb-2">A rotation of members; use it as a trigger recipient. Escalation steps notify a channel when nobody acknowledges in time.</p>
      {list.data && list.data.schedules.length > 0 && <ul className="mb-2 space-y-1 text-xs">{list.data.schedules.map((sc) => <li key={sc.id} className="flex items-center gap-2 flex-wrap"><span className="font-medium">{sc.name}</span><span className="text-muted">every {sc.rotation_days}d · {(sc as OncallSchedule & { member_emails?: string[] }).member_emails?.join(" → ")}</span>{sc.now && <Badge tone="ok">now: {sc.now.email}</Badge>}<span className="text-muted">{sc.escalation.length} escalation step(s)</span><button className="ml-auto text-muted hover:text-err" onClick={() => remove.mutate(sc.id)}><Trash2 size={12} /></button></li>)}</ul>}
      <form className="space-y-2 text-sm" onSubmit={async (e) => { e.preventDefault(); await create.mutateAsync(f); setF({ ...f, name: "", members: [], escalation: [] }); }}>
        <div className="grid grid-cols-3 gap-2"><div><Label>Name</Label><Input value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} required placeholder="primary" /></div><div><Label>Rotation days</Label><Input type="number" min={1} value={f.rotation_days} onChange={(e) => setF({ ...f, rotation_days: Number(e.target.value) })} /></div><div><Label>Starts on</Label><Input type="date" value={f.starts_on} onChange={(e) => setF({ ...f, starts_on: e.target.value })} /></div></div>
        <div><Label>Members (rotation order = click order)</Label><div className="flex flex-wrap gap-2 text-xs">{members.data?.members.map((m) => <label key={m.user_id} className="flex items-center gap-1"><input type="checkbox" checked={f.members.includes(m.user_id)} onChange={(e) => setF({ ...f, members: e.target.checked ? [...f.members, m.user_id] : f.members.filter((x) => x !== m.user_id) })} /> {m.email}</label>)}</div></div>
        <div><Label>Escalation</Label>{f.escalation.map((st, i) => <div key={i} className="flex items-center gap-2 text-xs mb-1"><span>after</span><Input type="number" className="w-20" value={st.after_secs} onChange={(e) => setF({ ...f, escalation: f.escalation.map((x, j) => (j === i ? { ...x, after_secs: Number(e.target.value) } : x)) })} /><span>s without ack →</span><Select value={st.channel_id} onChange={(e) => setF({ ...f, escalation: f.escalation.map((x, j) => (j === i ? { ...x, channel_id: e.target.value } : x)) })}>{channels.data?.channels.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}</Select><button type="button" className="text-muted hover:text-err" onClick={() => setF({ ...f, escalation: f.escalation.filter((_, j) => j !== i) })}><Trash2 size={12} /></button></div>)}<Button type="button" size="sm" variant="ghost" onClick={() => setF({ ...f, escalation: [...f.escalation, { after_secs: 600, channel_id: channels.data?.channels[0]?.id ?? "" }] })}>+ step</Button></div>
        <Button type="submit" variant="primary" size="sm" disabled={f.members.length === 0}>Add schedule</Button><ErrorBox error={create.error} />
      </form>
    </Card>
  );
}
