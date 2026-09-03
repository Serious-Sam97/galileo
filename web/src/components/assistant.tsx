"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import Link from "next/link";
import { usePathname } from "next/navigation";
import ReactMarkdown from "react-markdown";
import clsx from "clsx";
import { Sparkles, X, Send, ThumbsUp, ThumbsDown, Loader2 } from "lucide-react";
import { post, get } from "@/lib/api";
import { useProjectId, useMe } from "@/lib/hooks";
import { Button, Textarea } from "@/components/ui";

interface Step { action: string; summary: string; query_text?: string; link?: string }
interface Recording { project_id: string; span_id: string; trace_id: string; model: string }
interface Msg { role: "user" | "assistant"; content: string; steps?: Step[]; recording?: Recording; rated?: 1 | -1; error?: boolean }

const SUGGESTIONS = [
  "Which routes were slowest in the last 24h?",
  "Why is the slowest route slow? Look for N+1 queries.",
  "How many errors per exception type in the last hour, and which tenants are affected?",
  "What changed after the last deploy?",
  "Which LLM routes cost the most this week?",
];

/** Page context derived from the URL so the assistant knows what the user is looking at. */
function useContext(pid: string) {
  const path = usePathname() ?? "";
  const rest = path.replace(`/p/${pid}/`, "");
  const [page, ...parts] = rest.split("/");
  const ctx: Record<string, unknown> = { page: page || "overview", last_seconds: 86400 };
  if (page === "traces" && parts[0]) ctx.trace_id = parts[0];
  if (page === "issues" && parts[0]) ctx.issue_id = parts[0];
  if (page === "triggers" && parts[0] && parts[0] !== "form") ctx.trigger_id = parts[0];
  if (page === "browser" && parts[0] === "sessions" && parts[1]) ctx.session_id = parts[1];
  if (page === "query" && typeof window !== "undefined") {
    const q = new URLSearchParams(window.location.search).get("q");
    if (q) { try { ctx.query = JSON.parse(atob(q.replace(/-/g, "+").replace(/_/g, "/"))); } catch { /* ignore */ } }
  }
  return ctx;
}

export function Markdown({ children }: { children: string }) {
  return (
    <div className="prose-galileo text-[13px] leading-relaxed [&_h1]:text-sm [&_h2]:text-sm [&_h3]:text-[13px] [&_h1,&_h2,&_h3]:font-semibold [&_h1,&_h2,&_h3]:mt-2 [&_p]:my-1.5 [&_ul]:my-1 [&_ul]:list-disc [&_ul]:pl-4 [&_ol]:my-1 [&_ol]:list-decimal [&_ol]:pl-4 [&_li]:my-0.5 [&_code]:rounded [&_code]:bg-bg [&_code]:px-1 [&_code]:font-mono [&_code]:text-[11px] [&_pre]:my-1.5 [&_pre]:overflow-auto [&_pre]:rounded [&_pre]:border [&_pre]:bg-bg [&_pre]:p-2 [&_strong]:font-semibold [&_a]:text-info [&_a]:underline [&_table]:text-[11px] [&_th]:text-left [&_th]:pr-3 [&_td]:pr-3">
      <ReactMarkdown>{children}</ReactMarkdown>
    </div>
  );
}

export function AssistantDrawer() {
  const pid = useProjectId();
  const me = useMe();
  const ctx = useContext(pid);
  const [open, setOpen] = useState(false);
  const [msgs, setMsgs] = useState<Msg[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [enabled, setEnabled] = useState<boolean | null>(null);
  const [orgId, setOrgId] = useState<string | null>(null);
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "j") { e.preventDefault(); setOpen((o) => !o); } };
    window.addEventListener("keydown", onKey); return () => window.removeEventListener("keydown", onKey);
  }, []);
  useEffect(() => {
    const project = me.data?.projects.find((p) => p.id === pid);
    if (!project) return;
    setOrgId(project.org_id);
    get<{ assistant: { enabled: boolean } }>(`/api/orgs/${project.org_id}/assistant`).then((r) => setEnabled(!!r.assistant.enabled)).catch(() => setEnabled(false));
  }, [me.data, pid]);
  useEffect(() => { endRef.current?.scrollIntoView({ behavior: "smooth" }); }, [msgs, open]);

  const ask = useCallback(async (text: string) => {
    const q = text.trim(); if (!q || busy) return;
    const history = [...msgs, { role: "user" as const, content: q }];
    setMsgs(history); setInput(""); setBusy(true);
    try {
      const r = await post<{ answer: string; steps: Step[]; recording: Recording }>(`/api/projects/${pid}/assistant/chat`, { messages: history.map((m) => ({ role: m.role, content: m.content })), context: ctx });
      setMsgs([...history, { role: "assistant", content: r.answer, steps: r.steps, recording: r.recording }]);
    } catch (e) {
      setMsgs([...history, { role: "assistant", content: `Sorry — ${(e as Error).message}`, error: true }]);
    } finally { setBusy(false); }
  }, [busy, ctx, msgs, pid]);

  const rate = async (i: number, rating: 1 | -1) => {
    const m = msgs[i]; if (!m.recording) return;
    try { await post(`/api/projects/${m.recording.project_id}/gateway/feedback`, { span_id: m.recording.span_id, trace_id: m.recording.trace_id, rating, comment: "assistant answer" }); } catch { /* ignore */ }
    setMsgs(msgs.map((x, j) => (j === i ? { ...x, rated: rating } : x)));
  };

  return (
    <>
      <button onClick={() => setOpen(!open)} title="Ask Galileo (⌘J)" className={clsx("fixed bottom-5 right-5 z-40 flex h-11 w-11 items-center justify-center rounded-full shadow-lg transition", open ? "bg-panel-2 text-fg" : "bg-accent text-black hover:brightness-110")}>
        {open ? <X size={18} /> : <Sparkles size={18} />}
      </button>
      {open && (
        <div className="fixed bottom-20 right-5 z-40 flex h-[min(640px,80vh)] w-[420px] max-w-[calc(100vw-2.5rem)] flex-col rounded-xl border bg-panel shadow-2xl">
          <div className="flex items-center gap-2 border-b px-3 py-2 text-sm"><Sparkles size={14} className="text-accent" /><b>Ask Galileo</b><span className="text-muted text-xs">· {String(ctx.page)}{ctx.trace_id ? " · trace" : ctx.issue_id ? " · issue" : ""}</span>
            {msgs.length > 0 && <button className="ml-auto text-xs text-muted hover:text-fg" onClick={() => setMsgs([])}>clear</button>}</div>
          <div className="flex-1 overflow-auto scroll-thin p-3 space-y-3">
            {enabled === false && <div className="rounded border border-warn/40 bg-warn/10 p-2 text-xs">The assistant is not enabled for this organization. {orgId && <Link href={`/p/${pid}/settings`} className="text-info underline">Settings → Organization → Assistant</Link>} picks the gateway route that answers.</div>}
            {msgs.length === 0 && enabled !== false && (
              <div className="space-y-1.5">
                <p className="text-xs text-muted">Ask about this project&apos;s traces, logs, errors, users or LLM calls. Answers come from queries you can open and re-run.</p>
                {SUGGESTIONS.map((s) => <button key={s} onClick={() => ask(s)} className="block w-full rounded border bg-bg px-2 py-1.5 text-left text-xs hover:border-accent/60">{s}</button>)}
              </div>
            )}
            {msgs.map((m, i) => (
              <div key={i} className={clsx("rounded-lg px-3 py-2", m.role === "user" ? "ml-8 bg-accent/15" : m.error ? "mr-4 border border-err/40 bg-err/5" : "mr-4 border bg-bg")}>
                {m.role === "user" ? <div className="text-[13px] whitespace-pre-wrap">{m.content}</div> : <Markdown>{m.content}</Markdown>}
                {m.steps && m.steps.length > 0 && (
                  <div className="mt-2 flex flex-wrap gap-1">
                    {m.steps.map((s, j) => s.link ? <Link key={j} href={s.link} className="rounded border bg-panel px-1.5 py-0.5 font-mono text-[10px] text-info hover:underline" title={s.query_text ?? s.summary}>{s.action}{s.query_text ? `: ${s.query_text.slice(0, 60)}${s.query_text.length > 60 ? "…" : ""}` : ` · ${s.summary}`}</Link> : <span key={j} className="rounded border bg-panel px-1.5 py-0.5 font-mono text-[10px] text-muted" title={s.summary}>{s.action} · {s.summary}</span>)}
                  </div>
                )}
                {m.role === "assistant" && m.recording && (
                  <div className="mt-1.5 flex items-center gap-2 text-[10px] text-muted">
                    <span>{m.recording.model}</span>
                    <button onClick={() => rate(i, 1)} className={clsx("hover:text-ok", m.rated === 1 && "text-ok")} title="Good answer"><ThumbsUp size={12} /></button>
                    <button onClick={() => rate(i, -1)} className={clsx("hover:text-err", m.rated === -1 && "text-err")} title="Bad answer"><ThumbsDown size={12} /></button>
                    <Link href={`/p/${m.recording.project_id}/traces/${m.recording.trace_id}`} className="ml-auto hover:underline">calls →</Link>
                  </div>
                )}
              </div>
            ))}
            {busy && <div className="mr-4 flex items-center gap-2 rounded-lg border bg-bg px-3 py-2 text-xs text-muted"><Loader2 size={12} className="animate-spin" /> running queries…</div>}
            <div ref={endRef} />
          </div>
          <form className="flex items-end gap-2 border-t p-2" onSubmit={(e) => { e.preventDefault(); ask(input); }}>
            <Textarea rows={2} value={input} onChange={(e) => setInput(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); ask(input); } }} placeholder={enabled === false ? "enable the assistant first" : "Ask about this project… (Enter to send)"} disabled={busy || enabled === false} className="flex-1 text-[13px]" />
            <Button type="submit" variant="primary" size="sm" disabled={busy || !input.trim() || enabled === false}><Send size={13} /></Button>
          </form>
        </div>
      )}
    </>
  );
}

/** One-shot assistant actions used on issue/trigger/trace pages. */
export function AssistantAction({ label, run, onDone }: { label: string; run: () => Promise<{ markdown: string; recording?: Recording }>; onDone?: (md: string) => void }) {
  const [busy, setBusy] = useState(false);
  const [out, setOut] = useState<{ markdown: string; recording?: Recording } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [rated, setRated] = useState<1 | -1 | null>(null);
  return (
    <div className="space-y-2">
      <Button size="sm" onClick={async () => { setBusy(true); setErr(null); try { const r = await run(); setOut(r); onDone?.(r.markdown); } catch (e) { setErr((e as Error).message); } finally { setBusy(false); } }} disabled={busy}>
        {busy ? <Loader2 size={13} className="animate-spin" /> : <Sparkles size={13} className="text-accent" />} {busy ? "Thinking…" : label}
      </Button>
      {err && <p className="text-xs text-err">{err}</p>}
      {out && (
        <div className="rounded-lg border bg-bg p-3">
          <Markdown>{out.markdown}</Markdown>
          {out.recording && (
            <div className="mt-2 flex items-center gap-2 text-[10px] text-muted"><span>{out.recording.model}</span>
              <button onClick={async () => { await post(`/api/projects/${out.recording!.project_id}/gateway/feedback`, { span_id: out.recording!.span_id, trace_id: out.recording!.trace_id, rating: 1, comment: label }); setRated(1); }} className={clsx("hover:text-ok", rated === 1 && "text-ok")}><ThumbsUp size={12} /></button>
              <button onClick={async () => { await post(`/api/projects/${out.recording!.project_id}/gateway/feedback`, { span_id: out.recording!.span_id, trace_id: out.recording!.trace_id, rating: -1, comment: label }); setRated(-1); }} className={clsx("hover:text-err", rated === -1 && "text-err")}><ThumbsDown size={12} /></button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
