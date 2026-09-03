"use client";

import { useEffect, useMemo, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { useQuery } from "@tanstack/react-query";
import clsx from "clsx";
import { Search, CornerDownLeft } from "lucide-react";
import { get } from "@/lib/api";
import { useT } from "@/lib/i18n";

type SearchRes = {
  issues: { id: string; title: string; status: string }[];
  boards: { id: string; name: string }[];
  routes: { id: string; alias: string }[];
  triggers: { id: string; name: string; state: string }[];
  prompts: { id: string; name: string }[];
  users: { user_id: string; count: number }[];
  queries: { id: string; text: string }[];
};

type Item = { group: string; label: string; hint?: string; href?: string; run?: () => void; key: string };

const PAGES = ["overview", "query", "traces", "logs", "issues", "browser", "services-map", "metrics", "ai", "boards", "triggers", "slos", "settings", "changelog"];
const PAGE_LABEL: Record<string, string> = { "services-map": "Map", ai: "AI", slos: "SLOs", changelog: "What's new" };
export const GO_KEYS: Record<string, string> = { o: "overview", q: "query", t: "traces", l: "logs", i: "issues", b: "boards", a: "ai", s: "settings", m: "services-map", r: "triggers" };

export function CommandPalette({ pid, open, onClose, onAsk }: { pid: string; open: boolean; onClose: () => void; onAsk?: () => void }) {
  const t = useT();
  const router = useRouter();
  const [q, setQ] = useState("");
  const [ix, setIx] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => { if (open) { setQ(""); setIx(0); setTimeout(() => input.current?.focus(), 10); } }, [open]);
  const trimmed = q.trim();
  const isTrace = /^[0-9a-f]{32}$/i.test(trimmed);
  const res = useQuery({ queryKey: ["search", pid, trimmed], queryFn: () => get<SearchRes>(`/api/projects/${pid}/search?q=${encodeURIComponent(trimmed)}`), enabled: open && trimmed.length >= 2 && !isTrace, staleTime: 10_000 });

  const items: Item[] = useMemo(() => {
    const out: Item[] = [];
    const lc = trimmed.toLowerCase();
    if (isTrace) out.push({ group: t("Traces"), label: `${t("Open trace")} ${trimmed}`, href: `/p/${pid}/traces/${trimmed}`, key: "trace" });
    for (const p of PAGES) {
      const label = PAGE_LABEL[p] ?? p[0].toUpperCase() + p.slice(1);
      if (!lc || label.toLowerCase().includes(lc) || t(label).toLowerCase().includes(lc) || p.includes(lc)) out.push({ group: t("Go to"), label: t(label), hint: Object.entries(GO_KEYS).find(([, v]) => v === p)?.[0] ? `g ${Object.entries(GO_KEYS).find(([, v]) => v === p)![0]}` : undefined, href: `/p/${pid}/${p}`, key: `page:${p}` });
    }
    const acts: Item[] = [
      { group: t("Actions"), label: t("New trigger"), href: `/p/${pid}/triggers/new`, key: "a:trigger" },
      { group: t("Actions"), label: t("New board"), href: `/p/${pid}/boards?new=1`, key: "a:board" },
      { group: t("Actions"), label: t("Ask Galileo"), run: onAsk, hint: "⌘J", key: "a:ask" },
    ];
    for (const a of acts) if (!lc || a.label.toLowerCase().includes(lc)) out.push(a);
    const r = res.data;
    if (r) {
      r.issues.forEach((i) => out.push({ group: t("Issues"), label: i.title, hint: i.status, href: `/p/${pid}/issues/${i.id}`, key: `i:${i.id}` }));
      r.boards.forEach((b) => out.push({ group: t("Boards"), label: b.name, href: `/p/${pid}/boards/${b.id}`, key: `b:${b.id}` }));
      r.routes.forEach((x) => out.push({ group: t("Routes"), label: x.alias, href: `/p/${pid}/ai?tab=routes&route=${x.id}`, key: `r:${x.id}` }));
      r.triggers.forEach((x) => out.push({ group: t("Triggers"), label: x.name, hint: x.state, href: `/p/${pid}/triggers/${x.id}`, key: `t:${x.id}` }));
      r.prompts.forEach((x) => out.push({ group: t("Prompts"), label: x.name, href: `/p/${pid}/ai?tab=prompts&prompt=${x.id}`, key: `p:${x.id}` }));
      r.users.forEach((u) => out.push({ group: t("Users"), label: u.user_id, hint: `${u.count} spans`, href: `/p/${pid}/users/${encodeURIComponent(u.user_id)}`, key: `u:${u.user_id}` }));
      r.queries.forEach((x) => out.push({ group: t("Recent queries"), label: x.text, href: `/p/${pid}/query?text=${encodeURIComponent(x.text)}`, key: `q:${x.id}` }));
    }
    return out.slice(0, 40);
  }, [trimmed, isTrace, res.data, pid, t, onAsk]);

  useEffect(() => { setIx(0); }, [items.length, trimmed]);
  function go(it: Item) { onClose(); if (it.href) router.push(it.href); else it.run?.(); }
  if (!open) return null;
  let lastGroup = "";
  return (
    <div className="fixed inset-0 z-50 flex items-start justify-center bg-black/50 p-4 pt-[12vh]" onMouseDown={onClose}>
      <div className="w-full max-w-xl overflow-hidden rounded-xl border bg-panel shadow-2xl" onMouseDown={(e) => e.stopPropagation()} role="dialog" aria-label={t("Search")}>
        <div className="flex items-center gap-2 border-b px-3">
          <Search size={16} className="text-muted" />
          <input ref={input} value={q} onChange={(e) => setQ(e.target.value)} placeholder={t("Search or jump to…")} className="h-11 flex-1 bg-transparent text-sm outline-none"
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") { e.preventDefault(); setIx((i) => Math.min(i + 1, items.length - 1)); }
              else if (e.key === "ArrowUp") { e.preventDefault(); setIx((i) => Math.max(i - 1, 0)); }
              else if (e.key === "Enter") { e.preventDefault(); if (items[ix]) go(items[ix]); }
              else if (e.key === "Escape") onClose();
            }} />
          <kbd className="rounded border px-1 text-[10px] text-muted">esc</kbd>
        </div>
        <div className="max-h-[60vh] overflow-auto py-1">
          {items.length === 0 && <div className="px-3 py-6 text-center text-sm text-muted">{res.isFetching ? "…" : t("No data")}</div>}
          {items.map((it, i) => {
            const head = it.group !== lastGroup; lastGroup = it.group;
            return (
              <div key={it.key}>
                {head && <div className="px-3 pt-2 pb-1 text-[10px] uppercase tracking-wide text-muted">{it.group}</div>}
                <button onMouseEnter={() => setIx(i)} onClick={() => go(it)} className={clsx("flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm", i === ix ? "bg-accent/15 text-fg" : "text-fg/90 hover:bg-panel-2")}>
                  <span className="truncate flex-1">{it.label}</span>
                  {it.hint && <span className="text-[10px] text-muted font-mono">{it.hint}</span>}
                  {i === ix && <CornerDownLeft size={12} className="text-muted" />}
                </button>
              </div>
            );
          })}
        </div>
        <div className="flex items-center gap-3 border-t px-3 py-1.5 text-[10px] text-muted">
          <span>↑↓ navigate</span><span>↵ open</span><span>g + key jumps · ? shortcuts</span>
        </div>
      </div>
    </div>
  );
}

export function ShortcutsSheet({ open, onClose }: { open: boolean; onClose: () => void }) {
  const t = useT();
  if (!open) return null;
  const rows: [string, string][] = [["⌘K / Ctrl-K", t("Search")], ["⌘J", t("Ask Galileo")], ["g o / g q / g t / g l", "Overview / Query / Traces / Logs"], ["g i / g b / g a / g s", "Issues / Boards / AI / Settings"], ["g m / g r", "Map / Triggers"], ["/", "focus the page filter"], ["j / k, Enter", "move in lists, open"], ["Esc", "close drawers"], ["?", t("Keyboard shortcuts")]];
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4" onMouseDown={onClose}>
      <div className="w-full max-w-md rounded-xl border bg-panel p-4 shadow-2xl" onMouseDown={(e) => e.stopPropagation()}>
        <div className="mb-2 text-sm font-semibold">{t("Keyboard shortcuts")}</div>
        <table className="w-full text-sm">
          <tbody>{rows.map(([k, v]) => <tr key={k} className="border-t"><td className="py-1 pr-3 font-mono text-[11px] text-muted whitespace-nowrap">{k}</td><td className="py-1">{v}</td></tr>)}</tbody>
        </table>
      </div>
    </div>
  );
}

/** Global key handling: ⌘K, ?, g+key, / and j/k list navigation. */
export function useGlobalKeys(pid: string, opts: { openPalette: () => void; openHelp: () => void; closeAll: () => void }) {
  const router = useRouter();
  useEffect(() => {
    let pendingG = 0;
    function editing(e: KeyboardEvent) { const el = e.target as HTMLElement | null; return !!el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.isContentEditable); }
    function onKey(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") { e.preventDefault(); opts.openPalette(); return; }
      if (e.key === "Escape") { opts.closeAll(); return; }
      if (editing(e) || e.metaKey || e.ctrlKey || e.altKey) return;
      const now = Date.now();
      if (pendingG && now - pendingG < 1200) { pendingG = 0; const page = GO_KEYS[e.key.toLowerCase()]; if (page) { e.preventDefault(); router.push(`/p/${pid}/${page}`); } return; }
      if (e.key === "g") { pendingG = now; return; }
      if (e.key === "?") { e.preventDefault(); opts.openHelp(); return; }
      if (e.key === "/") { const f = document.querySelector<HTMLInputElement>("[data-page-filter]"); if (f) { e.preventDefault(); f.focus(); f.select(); } return; }
      if (e.key === "j" || e.key === "k" || e.key === "Enter") {
        const rows = Array.from(document.querySelectorAll<HTMLElement>("[data-row]"));
        if (rows.length === 0) return;
        let cur = rows.findIndex((r) => r.dataset.rowActive === "1");
        if (e.key === "Enter") { const link = (cur >= 0 ? rows[cur] : null)?.querySelector<HTMLAnchorElement>("a[href]") ?? (cur >= 0 ? (rows[cur] as HTMLAnchorElement) : null); if (link?.href) { e.preventDefault(); router.push(link.getAttribute("href")!); } return; }
        e.preventDefault();
        cur = e.key === "j" ? Math.min(cur + 1, rows.length - 1) : Math.max(cur - 1, 0);
        rows.forEach((r, i) => { if (i === cur) { r.dataset.rowActive = "1"; r.classList.add("row-active"); r.scrollIntoView({ block: "nearest" }); } else { delete r.dataset.rowActive; r.classList.remove("row-active"); } });
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [pid, router, opts]);
}
