"use client";

import { AssistantDrawer } from "@/components/assistant";
import { CommandPalette, ShortcutsSheet, useGlobalKeys } from "@/components/command-palette";
import { LATEST_VERSION } from "@/lib/changelog";
import { useT, useLocale, type Locale } from "@/lib/i18n";
import { RANGE_PRESETS, TimeRangeProvider, useLastSeconds } from "@/lib/time-range";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useCallback, useEffect, useMemo, useState } from "react";
import clsx from "clsx";
import { Activity, BarChart3, Bot, Bug, Gauge, LayoutDashboard, ListTree, ScrollText, Settings, Siren, Target, LogOut, Globe, Share2, Search, Menu, Sparkles, Clock } from "lucide-react";
import { useProjectQuery } from "@/lib/hooks";
import { useMe, useProjectId } from "@/lib/hooks";
import { post } from "@/lib/api";
import { Select } from "@/components/ui";
import { Logo } from "@/components/logo";

const NAV_GROUPS = [
  { label: "Watch", items: [
    { href: "overview", label: "Overview", icon: LayoutDashboard },
    { href: "issues", label: "Issues", icon: Bug },
    { href: "slos", label: "SLOs", icon: Target },
    { href: "triggers", label: "Triggers", icon: Siren },
  ] },
  { label: "Explore", items: [
    { href: "query", label: "Query", icon: BarChart3 },
    { href: "traces", label: "Traces", icon: ListTree },
    { href: "logs", label: "Logs", icon: ScrollText },
    { href: "metrics", label: "Metrics", icon: Gauge },
  ] },
  { label: "Product", items: [
    { href: "browser", label: "Browser", icon: Globe },
    { href: "ai", label: "AI", icon: Bot },
    { href: "services-map", label: "Map", icon: Share2 },
  ] },
  { label: "Build", items: [
    { href: "boards", label: "Boards", icon: Activity },
    { href: "settings", label: "Settings", icon: Settings },
  ] },
];
const NAV = NAV_GROUPS.flatMap((g) => g.items);

const SEEN_KEY = "galileo.changelog.seen";

export default function ProjectLayout({ children }: { children: React.ReactNode }) {
  return <TimeRangeProvider><Shell>{children}</Shell></TimeRangeProvider>;
}

function Shell({ children }: { children: React.ReactNode }) {
  const me = useMe();
  const pid = useProjectId();
  const path = usePathname();
  const router = useRouter();
  const t = useT();
  const [locale, setLocale] = useLocale();
  const [last, setLast] = useLastSeconds();
  const [palette, setPalette] = useState(false);
  const [help, setHelp] = useState(false);
  const [drawer, setDrawer] = useState(false);
  const [unseen, setUnseen] = useState(false);
  useEffect(() => {
    if (me.isError) router.replace("/login");
  }, [me.isError, router]);
  useEffect(() => {
    const check = () => { try { setUnseen(localStorage.getItem(SEEN_KEY) !== LATEST_VERSION); } catch { setUnseen(false); } };
    check(); window.addEventListener("galileo-changelog", check); return () => window.removeEventListener("galileo-changelog", check);
  }, []);
  useEffect(() => { setDrawer(false); }, [path]);
  const keyOpts = useMemo(() => ({
    openPalette: () => setPalette(true),
    openHelp: () => setHelp(true),
    closeAll: () => { setPalette(false); setHelp(false); setDrawer(false); },
  }), []);
  useGlobalKeys(pid, keyOpts);
  const openAssistant = useCallback(() => { window.dispatchEvent(new KeyboardEvent("keydown", { key: "j", metaKey: true })); }, []);
  const project = me.data?.projects.find((p) => p.id === pid);
  const org = me.data?.orgs.find((o) => o.id === project?.org_id);
  const section = path.split("/")[3] ?? "overview";
  const issueCounts = useProjectQuery<{ counts: Record<string, number> }>(["issue-counts"], "/issues?status=open&last_seconds=60", { refetchInterval: 60_000 });
  const openIssues = issueCounts.data?.counts?.open ?? 0;
  const sectionLabel = NAV.find((n) => n.href === section)?.label ?? (section === "changelog" ? "What's new" : section === "welcome" ? "Get started" : section);

  // The browser tab names the page and the project, and counts open issues: "(7) Overview · melea · Galileo".
  useEffect(() => {
    const parts = [t(sectionLabel), project?.name, "Galileo"].filter(Boolean);
    document.title = (openIssues > 0 ? `(${openIssues}) ` : "") + parts.join(" · ");
  }, [t, sectionLabel, project?.name, openIssues]);

  const navLink = (active: boolean) => clsx(
    "group mx-2 my-px flex min-h-[34px] items-center gap-2.5 rounded-lg px-2.5 text-[13px] transition-colors",
    active ? "nav-active font-semibold" : "text-muted hover:bg-panel-2 hover:text-fg",
  );

  return (
    <div className="flex min-h-screen">
      {drawer && <div className="fixed inset-0 z-30 md:hidden" onClick={() => setDrawer(false)} />}
      <aside className={clsx("sidebar flex w-56 shrink-0 flex-col border-r bg-[#120b22]/95", drawer && "open")}>
        <Link href={`/p/${pid}/overview`} className="flex items-center gap-2.5 px-4 pt-4 pb-3 text-[17px] font-bold tracking-tight">
          <Logo size={28} /> Galileo
        </Link>
        <div className="px-3 pb-2 space-y-1.5">
          <Select className="w-full" aria-label={t("Project")} value={pid} onChange={(e) => router.push(`/p/${e.target.value}/${section}`)}>
            {me.data?.projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
          </Select>
          {project && <Link href={`/org/${project.org_id}`} className="block px-1 text-[11.5px] text-faint hover:text-fg">← {t("All projects")}</Link>}
        </div>
        <button onClick={() => setPalette(true)} className="mx-3 mt-1 flex min-h-[36px] items-center gap-2 rounded-lg border bg-bg/60 px-2.5 text-xs text-muted hover:text-fg hover:border-accent/40">
          <Search size={14} /> <span className="flex-1 text-left">{t("Search")}</span> <kbd className="rounded-md border px-1.5 font-mono text-[10px]">⌘K</kbd>
        </button>
        <nav className="flex-1 overflow-auto scroll-thin py-1" aria-label={t("Main")}>
          {NAV_GROUPS.map((g) => (
            <div key={g.label} className="pt-3">
              <div className="px-5 pb-1 text-[10.5px] font-semibold uppercase tracking-[0.14em] text-faint">{t(g.label)}</div>
              {g.items.map((n) => {
                const active = section === n.href;
                const Icon = n.icon;
                return (
                  <Link key={n.href} href={`/p/${pid}/${n.href}`} aria-current={active ? "page" : undefined} className={navLink(active)}>
                    <Icon size={15} className={active ? "text-accent drop-shadow-[0_0_6px_rgb(255_92_207/0.7)]" : "text-faint group-hover:text-lilac"} /> {t(n.label)}
                    {n.href === "issues" && openIssues > 0 && <span className="ml-auto rounded-full bg-err/20 px-1.5 text-[10.5px] font-semibold text-err">{openIssues}</span>}
                  </Link>
                );
              })}
            </div>
          ))}
          <div className="pt-3">
            <Link href={`/p/${pid}/changelog`} className={navLink(section === "changelog")}>
              <Sparkles size={15} className={section === "changelog" ? "text-accent" : "text-faint"} /> {t("What's new")}
              {unseen && <span className="ml-auto h-2 w-2 rounded-full bg-accent shadow-[0_0_8px_var(--accent)]" title={LATEST_VERSION} />}
            </Link>
          </div>
        </nav>
        <div className="border-t px-3 py-3 space-y-2 text-xs text-muted">
          <div className="flex items-center gap-1">
            <span className="flex-1">{t("Language")}</span>
            <select value={locale} onChange={(e) => setLocale(e.target.value as Locale)} className="rounded-md border bg-panel px-1.5 py-0.5 text-[11px]" aria-label={t("Language")}>
              <option value="en">EN</option><option value="pt-BR">PT-BR</option>
            </select>
          </div>
          <div className="flex items-center gap-2">
            <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-panel-3 text-[12px] font-semibold text-fg">{(me.data?.user.email ?? "?").slice(0, 1).toUpperCase()}</span>
            <div className="min-w-0 flex-1">
              <div className="truncate text-fg">{me.data?.user.email}</div>
              {org && <div className="truncate text-[11px] text-faint">{org.name}</div>}
            </div>
            <button title={t("Sign out")} aria-label={t("Sign out")} onClick={async () => { await post("/api/auth/logout"); router.replace("/login"); }} className="rounded-md p-1 hover:text-fg hover:bg-panel-2"><LogOut size={14} /></button>
          </div>
        </div>
      </aside>
      <main className="flex min-w-0 flex-1 flex-col">
        <header className="sticky top-0 z-20 flex h-14 items-center justify-between gap-3 border-b bg-[#110a20]/85 px-3 backdrop-blur md:px-6 text-sm">
          <div className="flex items-center gap-2 min-w-0">
            <button className="md:hidden rounded-md p-1 text-muted hover:text-fg" onClick={() => setDrawer((d) => !d)} aria-label={t("Menu")}><Menu size={18} /></button>
            <div className="truncate text-muted">
              {org && <span className="hidden sm:inline">{org.name} <span className="mx-1 text-faint">/</span></span>}
              <span className="font-semibold text-fg">{project?.name ?? "…"}</span>
              <span className="mx-1.5 text-faint">/</span>
              <span className="text-fg">{t(sectionLabel)}</span>
            </div>
            <span className="ml-1 hidden items-center gap-1.5 rounded-full border border-ok/30 px-2.5 py-0.5 text-[11.5px] text-ok md:inline-flex" title={t("Pages refresh on their own")}>
              <span className="live-dot" /> {t("Live")}
            </span>
          </div>
          <div className="flex items-center gap-2">
            <label className="hidden items-center gap-1.5 sm:flex">
              <Clock size={14} className="text-faint" aria-hidden="true" />
              <span className="sr-only">{t("Time range")}</span>
              <Select value={last} onChange={(e) => setLast(Number(e.target.value))} className="min-h-[34px]">
                {RANGE_PRESETS.map(([s, l]) => <option key={s} value={s}>{t(l)}</option>)}
              </Select>
            </label>
            <button className="md:hidden rounded-md p-1 text-muted hover:text-fg" onClick={() => setPalette(true)} aria-label={t("Search")}><Search size={16} /></button>
            <button onClick={openAssistant} className="btn-vapor inline-flex min-h-[34px] items-center gap-1.5 rounded-lg px-3 text-[13px] font-semibold">
              <Sparkles size={14} /> <span className="hidden sm:inline">{t("Ask Galileo")}</span> <kbd className="hidden rounded border border-black/20 px-1 font-mono text-[10px] lg:inline">⌘J</kbd>
            </button>
            <button className="hidden md:inline rounded-md border px-2 py-0.5 text-[11px] text-muted hover:text-fg" onClick={() => setHelp(true)} title={t("Keyboard shortcuts")} aria-label={t("Keyboard shortcuts")}>?</button>
          </div>
        </header>
        <div key={section} className="page-pad rise flex-1 overflow-auto scroll-thin p-4 md:p-6">{children}</div>
        <AssistantDrawer />
        <CommandPalette pid={pid} open={palette} onClose={() => setPalette(false)} onAsk={openAssistant} />
        <ShortcutsSheet open={help} onClose={() => setHelp(false)} />
      </main>
    </div>
  );
}
