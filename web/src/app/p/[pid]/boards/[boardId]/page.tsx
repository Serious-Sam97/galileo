"use client";

import { useParams } from "next/navigation";
import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import { GridLayout, verticalCompactor, type Layout } from "react-grid-layout";
import { useProjectId, useProjectQuery, useProjectMutation, useRunQuery } from "@/lib/hooks";
import { put, patch, post, del, get, API_BASE } from "@/lib/api";
import { Button, Card, Empty, Drawer, ErrorBox, Select, Label, Input, Textarea, Badge, Table, Th, Td } from "@/components/ui";
import { SeriesCharts, HeatmapChart } from "@/components/query/result-charts";
import { GroupsTable } from "@/components/query/results";
import { TimeRangePicker } from "@/components/time-range";
import { Markdown } from "@/components/assistant";
import type { Board, Panel, PanelViz, SavedQuery, TimeRange, BoardVariable, Annotation, Issue, Query, Channel } from "@/lib/types";
import { encodeQ, fmtNum, fmtTime, ago } from "@/lib/format";
import { Plus, Trash2, ExternalLink, Settings2, Image as ImageIcon, MessageSquare, Pencil } from "lucide-react";

const COLS = 12;

/** Replace `$var` in filter values and breakdowns; drop filters whose variable is blank. */
export function substitute(q: Query, vars: Record<string, string>): Query {
  const s = JSON.stringify(q).replace(/\$([a-zA-Z_][a-zA-Z0-9_]*)/g, (m, n) => (n in vars ? vars[n] : m));
  const out = JSON.parse(s) as Query;
  out.filters = (out.filters ?? []).filter((f) => !(typeof f.value === "string" && (f.value === "" || f.value.startsWith("$"))));
  out.breakdowns = (out.breakdowns ?? []).filter((b) => b && !b.startsWith("$"));
  return out;
}

export default function BoardPage() {
  const { boardId } = useParams<{ boardId: string }>();
  const pid = useProjectId();
  const b = useProjectQuery<{ board: Board }>(["board", boardId], `/boards/${boardId}`);
  const saved = useProjectQuery<{ saved_queries: SavedQuery[] }>(["saved-queries"], "/saved-queries");
  const channels = useProjectQuery<{ channels: Channel[] }>(["channels"], "/channels");
  const ann = useProjectQuery<{ annotations: Annotation[] }>(["annotations", boardId], `/annotations?board_id=${boardId}&last_seconds=${30 * 86400}`);
  const [vars, setVars] = useState<Record<string, string>>({});
  const [editing, setEditing] = useState<Panel | null>(null);
  const [settings, setSettings] = useState(false);
  const [noteAt, setNoteAt] = useState<{ ts: number; panel: string } | null>(null);
  const [noteText, setNoteText] = useState("");
  const [sendTo, setSendTo] = useState("");
  const [sent, setSent] = useState("");
  const save = useProjectMutation<Board>((p, board) => put(`/api/projects/${p}/boards/${boardId}`, { name: board.name, description: board.description, panels: board.panels }), [["board", boardId], ["boards"]]);
  const saveSettings = useProjectMutation<{ variables?: BoardVariable[]; time_range?: TimeRange | null; compare?: boolean }>((p, s) => patch(`/api/projects/${p}/boards/${boardId}/settings`, s), [["board", boardId]]);
  const addNote = useProjectMutation<{ at: string; panel_id: string; text: string }>((p, n) => post(`/api/projects/${p}/annotations`, { kind: "chart", board_id: boardId, ...n }), [["annotations", boardId]]);
  const delNote = useProjectMutation<string>((p, id) => del(`/api/projects/${p}/annotations/${id}`), [["annotations", boardId]]);
  const board = b.data?.board;
  useEffect(() => { if (board?.variables) setVars((v) => { const n = { ...v }; for (const x of board.variables ?? []) if (!(x.name in n)) n[x.name] = x.default ?? ""; return n; }); }, [board?.variables]);
  const layout = useMemo<Layout>(() => (board?.panels ?? []).map((p, i) => ({ i: p.id, x: p.x ?? (i % 2) * 6, y: p.y ?? Math.floor(i / 2) * 4, w: p.w ?? 6, h: p.h ?? 4, minW: 2, minH: 2 })), [board?.panels]);
  if (b.error) return <ErrorBox error={b.error} />;
  if (!board) return <div className="text-muted">Loading…</div>;
  const range = board.time_range ?? null;
  const onLayout = (l: Layout) => {
    const panels = board.panels.map((p) => { const it = l.find((x) => x.i === p.id); return it ? { ...p, x: it.x, y: it.y, w: it.w, h: it.h } : p; });
    if (JSON.stringify(panels) !== JSON.stringify(board.panels)) save.mutate({ ...board, panels });
  };
  const upsertPanel = async (p: Panel) => { const exists = board.panels.some((x) => x.id === p.id); await save.mutateAsync({ ...board, panels: exists ? board.panels.map((x) => (x.id === p.id ? p : x)) : [...board.panels, { x: 0, y: Infinity as unknown as number, w: 6, h: 4, ...p }] }); setEditing(null); };
  const imageUrl = `/api/render?pid=${pid}&board=${boardId}${Object.entries(vars).map(([k, v]) => `&v_${k}=${encodeURIComponent(v)}`).join("")}`;
  const sendImage = async () => {
    if (!sendTo) return;
    setSent("…");
    const png = await fetch(imageUrl).then((r) => r.blob());
    const r = await fetch(`${API_BASE}/api/projects/${pid}/channels/${sendTo}/send-image?caption=${encodeURIComponent(`Board: ${board.name}`)}`, { method: "POST", credentials: "include", headers: { "content-type": "image/png" }, body: png });
    setSent(r.ok ? "sent" : `failed (${r.status})`);
  };
  return (
    <div className="mx-auto max-w-[1400px] space-y-4">
      <div className="flex items-center gap-3 flex-wrap">
        <Link href={`/p/${pid}/boards`} className="text-muted hover:text-fg text-xs">← boards</Link>
        <h1 className="text-xl font-semibold tracking-tight">{board.name}</h1>
        <span className="text-muted text-sm">{board.description}</span>
        {board.template && <Badge>{board.template}</Badge>}
        <div className="ml-auto flex items-center gap-2 flex-wrap">
          {(board.variables ?? []).map((v) => <VarPicker key={v.name} boardId={boardId} v={v} value={vars[v.name] ?? ""} onChange={(val) => setVars({ ...vars, [v.name]: val })} range={range} />)}
          <TimeRangePicker value={range ?? { last_seconds: 3600 }} onChange={(t) => saveSettings.mutate({ time_range: t })} />
          <label className="flex items-center gap-1 text-xs text-muted"><input type="checkbox" checked={!!board.compare} onChange={(e) => saveSettings.mutate({ compare: e.target.checked })} /> compare</label>
          <a href={imageUrl} target="_blank" rel="noreferrer" title="Render as PNG"><Button size="sm"><ImageIcon size={13} /> Image</Button></a>
          <Select value={sendTo} onChange={(e) => setSendTo(e.target.value)} className="text-xs"><option value="">send image to…</option>{channels.data?.channels.filter((c) => c.kind === "discord" || c.kind === "webhook").map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}</Select>
          {sendTo && <Button size="sm" onClick={sendImage}>{sent || "Send"}</Button>}
          <Button size="sm" onClick={() => setSettings(true)}><Settings2 size={13} /></Button>
          <Button size="sm" variant="primary" onClick={() => setEditing({ id: crypto.randomUUID(), title: "", viz: "line", query: saved.data?.saved_queries[0]?.query ?? { dataset: "spans", time_range: { last_seconds: 3600 }, calculations: [{ op: "COUNT" }], filters: [], breakdowns: [], orders: [] } })}><Plus size={13} /> Panel</Button>
        </div>
      </div>
      {board.panels.length === 0 ? <Empty>No panels yet. Add one, or create a board from a template.</Empty> : (
        <GridLayout className="layout" layout={layout} width={1180} gridConfig={{ cols: COLS, rowHeight: 64, margin: [12, 12] }} dragConfig={{ handle: ".panel-handle" }} compactor={verticalCompactor} onLayoutChange={onLayout}>
          {board.panels.map((p) => (
            <div key={p.id} className="rounded-xl border bg-panel/80 overflow-hidden flex flex-col">
              <div className="panel-handle flex items-center gap-2 border-b px-3 py-1.5 text-xs cursor-move select-none">
                <span className="font-medium truncate">{p.title || p.viz}</span>
                <span className="ml-auto flex items-center gap-2 text-muted">
                  {isQuery(p) && <Link href={`/p/${pid}/query?q=${encodeQ(withBoard(p.query as Query, vars, range, board.compare))}`} className="hover:text-fg" title="open in Query"><ExternalLink size={12} /></Link>}
                  <button className="hover:text-fg" onClick={() => setEditing(p)} title="edit"><Pencil size={12} /></button>
                  <button className="hover:text-err" onClick={() => save.mutate({ ...board, panels: board.panels.filter((x) => x.id !== p.id) })} title="remove"><Trash2 size={12} /></button>
                </span>
              </div>
              <div className="flex-1 min-h-0 overflow-auto scroll-thin p-2">
                <PanelBody panel={p} vars={vars} range={range} compare={!!board.compare} annotations={(ann.data?.annotations ?? []).filter((a) => a.at)} onPoint={(ts) => { setNoteAt({ ts, panel: p.id }); setNoteText(""); }} />
              </div>
            </div>
          ))}
        </GridLayout>
      )}
      {ann.data && ann.data.annotations.length > 0 && (
        <Card title={<span className="flex items-center gap-2"><MessageSquare size={13} /> Annotations</span>}>
          <ul className="space-y-1 text-xs">{ann.data.annotations.map((a) => <li key={a.id} className="flex items-start gap-2"><span className="text-muted whitespace-nowrap">{a.at ? fmtTime(a.at) : ago(a.created_at)}</span><span className="font-medium">{a.author ?? "?"}</span><span className="flex-1">{a.text}</span><button className="text-muted hover:text-err" onClick={() => delNote.mutate(a.id)}><Trash2 size={12} /></button></li>)}</ul>
        </Card>
      )}
      <Drawer open={!!noteAt} onClose={() => setNoteAt(null)} title={noteAt ? `Annotate ${fmtTime(new Date(noteAt.ts * 1000))}` : ""}>
        <div className="space-y-2">
          <p className="text-xs text-muted">Pinned to this time on every chart of the board. Mention a member with @their@email to notify them.</p>
          <Textarea rows={3} value={noteText} onChange={(e) => setNoteText(e.target.value)} placeholder="Deploy 1.4.3 went out here…" autoFocus />
          <ErrorBox error={addNote.error} />
          <Button variant="primary" onClick={async () => { if (!noteAt) return; await addNote.mutateAsync({ at: new Date(noteAt.ts * 1000).toISOString(), panel_id: noteAt.panel, text: noteText }); setNoteAt(null); }} disabled={!noteText.trim()}>Save note</Button>
        </div>
      </Drawer>
      <Drawer open={!!editing} onClose={() => setEditing(null)} title={editing && board.panels.some((x) => x.id === editing.id) ? "Edit panel" : "New panel"} width="w-[720px]">
        {editing && <PanelEditor panel={editing} saved={saved.data?.saved_queries ?? []} onSave={upsertPanel} error={save.error} />}
      </Drawer>
      <Drawer open={settings} onClose={() => setSettings(false)} title="Board settings" width="w-[640px]">
        <BoardSettings board={board} onSave={async (s) => { await saveSettings.mutateAsync(s); setSettings(false); }} onRename={async (name, description) => { await save.mutateAsync({ ...board, name, description }); }} error={saveSettings.error} />
      </Drawer>
    </div>
  );
}

function isQuery(p: Panel): p is Panel & { query: Query } { return !!(p.query as Query)?.dataset && !["markdown", "issues", "slo", "service_map"].includes(p.viz ?? ""); }
function withBoard(q: Query, vars: Record<string, string>, range: TimeRange | null, compare?: boolean): Query {
  const out = substitute(q, vars);
  if (range) out.time_range = range;
  if (compare) out.compare_to = "previous";
  return out;
}

function VarPicker({ boardId, v, value, onChange, range }: { boardId: string; v: BoardVariable; value: string; onChange: (s: string) => void; range: TimeRange | null }) {
  const last = range && "last_seconds" in range ? range.last_seconds : 86400;
  const vals = useProjectQuery<{ values: { value: string; count: number }[] }>(["board-var", boardId, v.name, last], `/boards/${boardId}/variables/${encodeURIComponent(v.name)}/values?last_seconds=${last}`);
  return (
    <label className="flex items-center gap-1 text-xs"><span className="text-muted">{v.label || v.name}</span>
      <Select value={value} onChange={(e) => onChange(e.target.value)} className="text-xs"><option value="">all</option>{(vals.data?.values ?? []).map((x) => <option key={x.value} value={x.value}>{x.value} ({fmtNum(x.count, 0)})</option>)}{value && !vals.data?.values.some((x) => x.value === value) && <option value={value}>{value}</option>}</Select>
    </label>
  );
}

function PanelBody({ panel, vars, range, compare, annotations, onPoint }: { panel: Panel; vars: Record<string, string>; range: TimeRange | null; compare: boolean; annotations: Annotation[]; onPoint: (ts: number) => void }) {
  const pid = useProjectId();
  const viz = panel.viz ?? "line";
  if (viz === "markdown") return <Markdown>{String((panel.query as { markdown?: string }).markdown ?? "")}</Markdown>;
  if (viz === "issues") return <IssuesPanel />;
  if (viz === "slo") return <SloPanel id={String((panel.query as { slo_id?: string }).slo_id ?? "")} />;
  if (viz === "service_map") return <div className="text-xs text-muted">Service map: <Link href={`/p/${pid}/services-map`} className="text-info hover:underline">open the Map page</Link> (inline rendering lands with the next boards update).</div>;
  return <QueryPanel panel={panel as Panel & { query: Query }} vars={vars} range={range} compare={compare} annotations={annotations} onPoint={onPoint} />;
}

function QueryPanel({ panel, vars, range, compare, annotations, onPoint }: { panel: Panel & { query: Query }; vars: Record<string, string>; range: TimeRange | null; compare: boolean; annotations: Annotation[]; onPoint: (ts: number) => void }) {
  const q = withBoard(panel.query, vars, range, compare);
  const res = useRunQuery(q, { refetchInterval: 60_000 });
  const viz = panel.viz ?? "line";
  if (res.error) return <ErrorBox error={res.error} />;
  if (!res.data) return <div className="text-xs text-muted">Loading…</div>;
  if (viz === "stat") { const g = res.data.groups[0]; return <div className="text-3xl font-semibold tabular-nums">{fmtNum(g?.totals[0] ?? null)}{g?.compare_totals?.[0] != null && <span className="ml-2 text-sm text-muted font-normal">prev {fmtNum(g.compare_totals[0])}</span>}<div className="text-xs text-muted font-normal">{res.data.calculations[0]}</div></div>; }
  if (viz === "table") return <GroupsTable res={res.data} />;
  if (viz === "heatmap") return res.data.heatmap ? <HeatmapChart res={res.data} /> : <Empty>Add a HEATMAP calculation to this panel&apos;s query.</Empty>;
  if (!res.data.groups.length) return <Empty>No data</Empty>;
  return <SeriesCharts res={res.data} annotations={annotations.map((a) => ({ ts: new Date(a.at!).getTime() / 1000, text: a.text }))} onPointClick={onPoint} height={200} />;
}

function IssuesPanel() {
  const pid = useProjectId();
  const q = useProjectQuery<{ issues: Issue[] }>(["issues-panel"], "/issues?status=open&last_seconds=604800");
  if (!q.data) return null;
  if (!q.data.issues.length) return <Empty>No open issues.</Empty>;
  return <Table><thead><tr><Th>Issue</Th><Th>Route</Th><Th className="text-right">Count</Th><Th>Last</Th></tr></thead><tbody>{q.data.issues.slice(0, 8).map((i) => <tr key={i.id}><Td><Link href={`/p/${pid}/issues/${i.id}`} className="hover:underline">{i.title.slice(0, 60)}</Link></Td><Td className="font-mono text-muted">{i.route}</Td><Td className="text-right">{fmtNum(i.count, 0)}</Td><Td className="text-muted">{ago(i.last_seen)}</Td></tr>)}</tbody></Table>;
}

function SloPanel({ id }: { id: string }) {
  const q = useProjectQuery<{ slo: { name: string; target: number; sli?: number; error_budget_remaining?: number; burn_rate_1h?: number } }>(["slo-panel", id], `/slos/${id}`, { enabled: !!id });
  if (!id) return <Empty>Pick an SLO in the panel editor.</Empty>;
  const s = q.data?.slo; if (!s) return null;
  return <div className="space-y-1"><div className="font-medium">{s.name}</div><div className="text-2xl font-semibold tabular-nums">{s.sli != null ? `${(s.sli * 100).toFixed(2)}%` : "–"} <span className="text-xs text-muted font-normal">target {(s.target * 100).toFixed(2)}%</span></div><div className="text-xs text-muted">budget remaining {s.error_budget_remaining != null ? `${(s.error_budget_remaining * 100).toFixed(0)}%` : "–"} · burn 1h {s.burn_rate_1h?.toFixed(2) ?? "–"}</div></div>;
}

function PanelEditor({ panel, saved, onSave, error }: { panel: Panel; saved: SavedQuery[]; onSave: (p: Panel) => Promise<void>; error: unknown }) {
  const pid = useProjectId();
  const [p, setP] = useState<Panel>(panel);
  const [text, setText] = useState("");
  const [parseErr, setParseErr] = useState("");
  const slos = useProjectQuery<{ slos: { id: string; name: string }[] }>(["slos"], "/slos");
  const isQ = !["markdown", "issues", "slo", "service_map"].includes(p.viz ?? "line");
  return (
    <div className="space-y-3 text-sm">
      <div className="grid grid-cols-2 gap-2">
        <div><Label>Title</Label><Input value={p.title} onChange={(e) => setP({ ...p, title: e.target.value })} /></div>
        <div><Label>Type</Label><Select className="w-full" value={p.viz ?? "line"} onChange={(e) => setP({ ...p, viz: e.target.value as PanelViz })}>{(["line", "table", "stat", "heatmap", "markdown", "issues", "slo", "service_map"] as PanelViz[]).map((v) => <option key={v} value={v}>{v}</option>)}</Select></div>
      </div>
      {p.viz === "markdown" && <div><Label>Markdown</Label><Textarea rows={6} value={String((p.query as { markdown?: string }).markdown ?? "")} onChange={(e) => setP({ ...p, query: { markdown: e.target.value } })} /></div>}
      {p.viz === "slo" && <div><Label>SLO</Label><Select className="w-full" value={String((p.query as { slo_id?: string }).slo_id ?? "")} onChange={(e) => setP({ ...p, query: { slo_id: e.target.value } })}><option value="">choose…</option>{slos.data?.slos.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}</Select></div>}
      {isQ && (
        <>
          <div><Label>From a saved query</Label><Select className="w-full" value="" onChange={(e) => { const sq = saved.find((s) => s.id === e.target.value); if (sq) setP({ ...p, query: sq.query, title: p.title || sq.name }); }}><option value="">choose…</option>{saved.map((s) => <option key={s.id} value={s.id}>{s.name}</option>)}</Select></div>
          <div><Label>…or as text DSL (use <code>$variable</code> in values)</Label><Textarea rows={3} className="font-mono text-[12px]" value={text} onChange={(e) => setText(e.target.value)} placeholder={'spans | where service_name = $service | p95(duration_ms), count() by http_route | limit 10'} />
            <div className="flex gap-2 mt-1"><Button size="sm" onClick={async () => { const r = await post<{ ok: boolean; query?: Query; error?: string }>(`/api/projects/${pid}/query/parse`, { text: text.replace(/\$([a-zA-Z_]+)/g, "__VAR_$1__") }); if (r.ok && r.query) { setP({ ...p, query: JSON.parse(JSON.stringify(r.query).replace(/__VAR_([a-zA-Z_]+)__/g, "$$$1")) }); setParseErr(""); } else setParseErr(r.error ?? "parse error"); }}>Parse</Button>
              <Button size="sm" onClick={async () => { const r = await post<{ text: string }>(`/api/projects/${pid}/query/stringify`, { query: p.query }); setText(r.text); }}>Show current</Button>{parseErr && <span className="text-xs text-err">{parseErr}</span>}</div></div>
          <pre className="max-h-40 overflow-auto scroll-thin rounded border bg-bg p-2 font-mono text-[10px] text-muted">{JSON.stringify(p.query, null, 1)}</pre>
        </>
      )}
      <ErrorBox error={error} />
      <Button variant="primary" onClick={() => onSave(p)}>Save panel</Button>
    </div>
  );
}

function BoardSettings({ board, onSave, onRename, error }: { board: Board; onSave: (s: { variables?: BoardVariable[]; time_range?: TimeRange | null; compare?: boolean }) => Promise<void>; onRename: (n: string, d: string) => Promise<void>; error: unknown }) {
  const [name, setName] = useState(board.name); const [desc, setDesc] = useState(board.description);
  const [vars, setVars] = useState<BoardVariable[]>(board.variables ?? []);
  return (
    <div className="space-y-3 text-sm">
      <div className="grid grid-cols-2 gap-2"><div><Label>Name</Label><Input value={name} onChange={(e) => setName(e.target.value)} /></div><div><Label>Description</Label><Input value={desc} onChange={(e) => setDesc(e.target.value)} /></div></div>
      <div className="flex items-center justify-between"><Label>Variables (use as <code>$name</code> in panel queries)</Label><Button size="sm" onClick={() => setVars([...vars, { name: "", field: "", label: "", default: "" }])}><Plus size={12} /></Button></div>
      {vars.map((v, i) => (
        <div key={i} className="grid grid-cols-[1fr_1fr_1fr_1fr_auto] gap-2 items-end">
          <div><Label>name</Label><Input className="font-mono" value={v.name} onChange={(e) => setVars(vars.map((x, j) => (j === i ? { ...x, name: e.target.value.replace(/[^a-zA-Z0-9_]/g, "") } : x)))} placeholder="tenant" /></div>
          <div><Label>field</Label><Input className="font-mono" value={v.field} onChange={(e) => setVars(vars.map((x, j) => (j === i ? { ...x, field: e.target.value } : x)))} placeholder="tenant_id" /></div>
          <div><Label>label</Label><Input value={v.label ?? ""} onChange={(e) => setVars(vars.map((x, j) => (j === i ? { ...x, label: e.target.value } : x)))} /></div>
          <div><Label>default</Label><Input value={v.default ?? ""} onChange={(e) => setVars(vars.map((x, j) => (j === i ? { ...x, default: e.target.value } : x)))} /></div>
          <button className="text-muted hover:text-err pb-2" onClick={() => setVars(vars.filter((_, j) => j !== i))}><Trash2 size={13} /></button>
        </div>
      ))}
      <ErrorBox error={error} />
      <div className="flex gap-2"><Button variant="primary" onClick={async () => { await onRename(name, desc); await onSave({ variables: vars.filter((v) => v.name && v.field) }); }}>Save</Button>
        <Button onClick={() => onSave({ time_range: null })}>Use each panel&apos;s own range</Button></div>
    </div>
  );
}
