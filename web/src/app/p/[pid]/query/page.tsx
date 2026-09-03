"use client";

import { useCallback, useEffect, useState } from "react";
import { QueryBuilder } from "@/components/query/builder";
import { SeriesCharts, HeatmapChart } from "@/components/query/result-charts";
import { GroupsTable, RawTable, ResultStats } from "@/components/query/results";
import { BubbleUp, BubbleUpBar } from "@/components/query/bubbleup";
import { useProjectId, useRunQuery, useProjectMutation, useProjectQuery } from "@/lib/hooks";
import { post } from "@/lib/api";
import { defaultQuery, type Filter, type Query } from "@/lib/types";
import { decodeQ, encodeQ } from "@/lib/format";
import { Button, Drawer, ErrorBox, Input, Label, Textarea } from "@/components/ui";
import { API_BASE, post as apiPost } from "@/lib/api";
import { Code2, Download, Share2, Terminal, History as HistoryIcon } from "lucide-react";
import type { QueryHistoryRow, Annotation } from "@/lib/types";

export default function QueryPage() {
  const pid = useProjectId();
  // The query lives in the URL (?q=) so results are shareable, but we read and write it with
  // the History API directly: a Next navigation per run would re-suspend the page.
  const [draft, setDraft] = useState<Query | null>(null);
  const [active, setActive] = useState<Query | null>(null);
  const [selection, setSelection] = useState<Filter[]>([]);
  const [showSql, setShowSql] = useState(false);
  const [saving, setSaving] = useState(false);
  const [share, setShare] = useState<string | null>(null);
  const [dsl, setDsl] = useState("");
  const [dslErr, setDslErr] = useState("");
  const [showDsl, setShowDsl] = useState(false);
  const [showHistory, setShowHistory] = useState(false);
  const [queryText, setQueryText] = useState("");
  const [noteAt, setNoteAt] = useState<number | null>(null);
  const [noteText, setNoteText] = useState("");
  const history = useProjectQuery<{ history: QueryHistoryRow[] }>(["query-history"], "/query/history", { enabled: showHistory });
  const notes = useProjectQuery<{ annotations: Annotation[] }>(["annotations-q", queryText], `/annotations?query_text=${encodeURIComponent(queryText)}&last_seconds=${30 * 86400}`, { enabled: !!queryText });
  const addNote = useProjectMutation<{ at: string; text: string }>((p, n) => apiPost(`/api/projects/${p}/annotations`, { kind: "query", query_text: queryText, ...n }), [["annotations-q", queryText]]);
  const res = useRunQuery(active);

  useEffect(() => {
    const q = decodeQ<Query>(new URLSearchParams(window.location.search).get("q")) ?? defaultQuery();
    setDraft(q);
    setActive(q);
  }, []);
  useEffect(() => {
    if (!active) return;
    const url = `/p/${pid}/query?q=${encodeQ(active)}`;
    if (window.location.pathname + window.location.search !== url) window.history.replaceState(null, "", url);
  }, [active, pid]);

  const run = useCallback(() => { setActive(draft); setSelection([]); }, [draft]);
  useEffect(() => { if (!active) return; apiPost<{ text: string }>(`/api/projects/${pid}/query/stringify`, { query: active }).then((r) => setQueryText(r.text)).catch(() => {}); }, [active, pid]);

  const onHeatmapBrush = useCallback((sel: { tStart: number; tEnd: number; lo: number; hi: number }) => {
    const field = active?.calculations.find((c) => c.op === "HEATMAP")?.field ?? "duration_ms";
    const f: Filter[] = [
      { field: "timestamp", op: "gte", value: new Date(sel.tStart * 1000).toISOString() },
      { field: "timestamp", op: "lt", value: new Date(sel.tEnd * 1000).toISOString() },
      { field, op: "gte", value: Number(sel.lo.toPrecision(4)) },
    ];
    if (Number.isFinite(sel.hi)) f.push({ field, op: "lt", value: Number(sel.hi.toPrecision(4)) });
    setSelection(f);
  }, [active]);

  const onSeriesBrush = useCallback((s: number, e: number) => {
    setSelection([
      { field: "timestamp", op: "gte", value: new Date(s * 1000).toISOString() },
      { field: "timestamp", op: "lt", value: new Date(e * 1000).toISOString() },
    ]);
  }, []);

  const save = useProjectMutation<{ name: string; description: string }>((p, b) => post(`/api/projects/${p}/saved-queries`, { ...b, query: active }), [["saved-queries"]]);

  const downloadCsv = useCallback(async () => {
    if (!active) return;
    const r = await fetch(`${API_BASE}/api/projects/${pid}/query/export`, { method: "POST", credentials: "include", headers: { "content-type": "application/json" }, body: JSON.stringify(active) });
    const blob = await r.blob();
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a"); a.href = url; a.download = `galileo-${active.dataset}-${Date.now()}.csv`; a.click();
    setTimeout(() => URL.revokeObjectURL(url), 5000);
  }, [active, pid]);

  const makeShare = useCallback(async () => {
    if (!active) return;
    const r = await apiPost<{ slug: string }>(`/api/projects/${pid}/shares`, { query: active });
    const link = `${window.location.origin}/p/${pid}/share/${r.slug}`;
    try { await navigator.clipboard.writeText(link); } catch { /* clipboard may be blocked */ }
    setShare(link);
  }, [active, pid]);

  const openDsl = useCallback(() => { setDsl(""); setDslErr(""); setShowDsl(true); }, []);
  const applyDsl = useCallback(async () => {
    const r = await apiPost<{ ok: boolean; query?: Query; error?: string }>(`/api/projects/${pid}/query/parse`, { text: dsl });
    if (r.ok && r.query) { setDraft(r.query); setActive(r.query); setShowDsl(false); } else { setDslErr(r.error ?? "parse error"); }
  }, [dsl, pid]);

  if (!draft || !active) return null;
  return (
    <div className="space-y-3">
      <QueryBuilder query={draft} onChange={setDraft} onRun={run} onSave={() => setSaving(true)} running={res.isFetching} />
      <ErrorBox error={res.error} />
      {res.data && (
        <>
          <div className="flex items-center justify-between">
            <ResultStats res={res.data} />
            <div className="flex items-center gap-1">
              <Button size="sm" variant="ghost" onClick={() => setShowHistory(true)}><HistoryIcon size={13} /> History</Button>
              <Button size="sm" variant="ghost" onClick={openDsl}><Terminal size={13} /> Text</Button>
              <Button size="sm" variant="ghost" onClick={downloadCsv}><Download size={13} /> CSV</Button>
              <Button size="sm" variant="ghost" onClick={makeShare}><Share2 size={13} /> Share</Button>
              <Button size="sm" variant="ghost" onClick={() => setShowSql(true)}><Code2 size={13} /> SQL</Button>
            </div>
          </div>
          {res.data.mode === "heatmap" && <HeatmapChart res={res.data} onBrush={onHeatmapBrush} />}
          {res.data.groups.length > 0 && res.data.calculations.length > 0 && <SeriesCharts res={res.data} onBrush={onSeriesBrush} annotations={(notes.data?.annotations ?? []).filter((a) => a.at).map((a) => ({ ts: new Date(a.at!).getTime() / 1000, text: a.text }))} onPointClick={(ts) => { setNoteAt(ts); setNoteText(""); }} />}
          {res.data.mode === "series" && <GroupsTable res={res.data} />}
          {res.data.mode === "raw" && <RawTable res={res.data} />}
          {res.data.mode !== "raw" && <BubbleUpBar defaultField={active.calculations.find((c) => c.field)?.field ?? (active.dataset === "spans" ? "duration_ms" : "value")} onSelect={(f) => setSelection([f])} />}
          <BubbleUp query={active} selection={selection} onClear={() => setSelection([])} onAddFilter={(f) => { const q = { ...draft, filters: [...draft.filters, f] }; setDraft(q); setActive(q); setSelection([]); }} />
        </>
      )}
      <Drawer open={showSql} onClose={() => setShowSql(false)} title="Generated SQL" width="w-[720px]">
        {res.data?.sql.map((s, i) => <pre key={i} className="mb-3 whitespace-pre-wrap rounded-md border bg-bg p-3 font-mono text-[11px] text-muted">{s}</pre>)}
      </Drawer>
      <Drawer open={saving} onClose={() => setSaving(false)} title="Save query">
        <SaveForm onSave={async (b) => { await save.mutateAsync(b); setSaving(false); }} error={save.error} />
      </Drawer>
      <Drawer open={showDsl} onClose={() => setShowDsl(false)} title="Query as text" width="w-[640px]">
        <div className="space-y-2">
          <p className="text-xs text-muted">Pipe form of the DSL. Example:</p>
          <pre className="rounded border bg-bg p-2 font-mono text-[11px] text-muted whitespace-pre-wrap">spans | where http.route = &quot;/api/x&quot; and status_code = error | p95(duration_ms), count() by tenant_id | having count() &gt; 100 | order p95 desc | limit 20 | compare previous</pre>
          <Textarea rows={4} className="font-mono" value={dsl} onChange={(e) => setDsl(e.target.value)} placeholder={active ? "" : "spans | count() by service_name"} autoFocus />
          {dslErr && <p className="text-xs text-err">{dslErr}</p>}
          <div className="flex gap-2"><Button variant="primary" size="sm" onClick={applyDsl}>Parse &amp; run</Button>
            <Button size="sm" onClick={async () => { const r = await apiPost<{ text: string }>(`/api/projects/${pid}/query/stringify`, { query: active }); setDsl(r.text); setDslErr(""); }}>Show current query as text</Button></div>
        </div>
      </Drawer>
      <Drawer open={showHistory} onClose={() => setShowHistory(false)} title="Query history" width="w-[640px]">
        {history.data && (history.data.history.length === 0 ? <p className="text-sm text-muted">No runs yet.</p> : (
          <ul className="space-y-1">{history.data.history.map((h) => <li key={h.id} className="flex items-start gap-2 rounded border p-2 text-xs"><span className="text-muted whitespace-nowrap">{new Date(h.ran_at).toLocaleString()}</span><code className="flex-1 font-mono whitespace-pre-wrap">{h.text}</code><Button size="sm" onClick={() => { setDraft(h.query); setActive(h.query); setShowHistory(false); }}>run</Button></li>)}</ul>
        ))}
      </Drawer>
      <Drawer open={noteAt != null} onClose={() => setNoteAt(null)} title={noteAt != null ? `Annotate ${new Date(noteAt * 1000).toLocaleString()}` : ""}>
        <div className="space-y-2">
          <p className="text-xs text-muted">Pinned to this time for this query (matched by its text). Mention a member with @their@email to notify them.</p>
          <Textarea rows={3} value={noteText} onChange={(e) => setNoteText(e.target.value)} autoFocus />
          <ErrorBox error={addNote.error} />
          <Button variant="primary" disabled={!noteText.trim()} onClick={async () => { if (noteAt == null) return; await addNote.mutateAsync({ at: new Date(noteAt * 1000).toISOString(), text: noteText }); setNoteAt(null); }}>Save note</Button>
        </div>
      </Drawer>
      <Drawer open={!!share} onClose={() => setShare(null)} title="Share link">
        <p className="text-sm text-muted mb-2">The time range is frozen to what you ran. Anyone with project access can open it.</p>
        <Input readOnly value={share ?? ""} onFocus={(e) => e.currentTarget.select()} className="font-mono" />
        <p className="mt-2 text-xs text-muted">Copied to your clipboard.</p>
      </Drawer>
    </div>
  );
}

function SaveForm({ onSave, error }: { onSave: (b: { name: string; description: string }) => Promise<void>; error: unknown }) {
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  return (
    <form className="space-y-3" onSubmit={(e) => { e.preventDefault(); onSave({ name, description }); }}>
      <div><Label>Name</Label><Input value={name} onChange={(e) => setName(e.target.value)} required autoFocus /></div>
      <div><Label>Description</Label><Textarea value={description} onChange={(e) => setDescription(e.target.value)} rows={3} /></div>
      <ErrorBox error={error} />
      <Button type="submit" variant="primary">Save</Button>
    </form>
  );
}
