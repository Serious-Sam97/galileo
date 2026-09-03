"use client";

import { AssistantDrawer } from "@/components/assistant";
import { CommandPalette, ShortcutsSheet, useGlobalKeys } from "@/components/command-palette";
import { LATEST_VERSION } from "@/lib/changelog";
import { useT, useLocale, type Locale } from "@/lib/i18n";
import { useTheme, type Theme } from "@/lib/theme";

import Link from "next/link";
import { usePathname, useRouter } from "next/navigation";
import { useCallback, useEffect, useMemo, useState } from "react";
import clsx from "clsx";
import { Activity, BarChart3, Bot, Bug, Gauge, LayoutDashboard, ListTree, ScrollText, Settings, Siren, Target, Telescope, LogOut, Globe, Share2, Search, Menu, Sparkles, Sun, Moon, Monitor } from "lucide-react";
import { useProjectQuery } from "@/lib/hooks";
import { useMe, useProjectId } from "@/lib/hooks";
import { post } from "@/lib/api";
import { Select } from "@/components/ui";

const NAV = [
  { href: "overview", label: "Overview", icon: LayoutDashboard },
  { href: "query", label: "Query", icon: BarChart3 },
  { href: "traces", label: "Traces", icon: ListTree },
  { href: "logs", label: "Logs", icon: ScrollText },
  { href: "issues", label: "Issues", icon: Bug },
  { href: "browser", label: "Browser", icon: Globe },
  { href: "services-map", label: "Map", icon: Share2 },
  { href: "metrics", label: "Metrics", icon: Gauge },
  { href: "ai", label: "AI", icon: Bot },
  { href: "boards", label: "Boards", icon: Activity },
  { href: "triggers", label: "Triggers", icon: Siren },
  { href: "slos", label: "SLOs", icon: Target },
  { href: "settings", label: "Settings", icon: Settings },
];

const SEEN_KEY = "galileo.changelog.seen";

export default function ProjectLayout({ children }: { children: React.ReactNode }) {
  const me = useMe();
  const pid = useProjectId();
  const path = usePathname();
  const router = useRouter();
  const t = useT();
  const [locale, setLocale] = useLocale();
  const [theme, setTheme, resolved] = useTheme();
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
  const section = path.split("/")[3] ?? "overview";
  const issueCounts = useProjectQuery<{ counts: Record<string, number> }>(["issue-counts"], "/issues?status=open&last_seconds=60", { refetchInterval: 60_000 });
  const openIssues = issueCounts.data?.counts?.open ?? 0;
  const sectionLabel = NAV.find((n) => n.href === section)?.label ?? (section === "changelog" ? "What's new" : section === "welcome" ? "Get started" : section);

  return (
    <div className="flex min-h-screen">
      {drawer && <div className="fixed inset-0 z-30 md:hidden" onClick={() => setDrawer(false)} />}
      <aside className={clsx("sidebar flex w-52 shrink-0 flex-col border-r bg-panel", drawer && "open")}>
        <div className="flex items-center gap-2 px-4 py-3 text-base font-semibold border-b"><Telescope className="text-accent" size={20} /> Galileo</div>
        <div className="px-3 py-2 border-b space-y-1.5">
          <Select className="w-full" value={pid} onChange={(e) => router.push(`/p/${e.target.value}/${section}`)}>
            {me.data?.projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
          </Select>
          {project && <Link href={`/org/${project.org_id}`} className="block text-[11px] text-muted hover:text-fg">← {t("All projects")}</Link>}
        </div>
        <button onClick={() => setPalette(true)} className="mx-3 mt-2 flex items-center gap-2 rounded-md border px-2 py-1.5 text-xs text-muted hover:text-fg hover:bg-panel-2">
          <Search size={13} /> <span className="flex-1 text-left">{t("Search")}</span> <kbd className="rounded border px-1 text-[10px]">⌘K</kbd>
        </button>
        <nav className="flex-1 py-2">
          {NAV.map((n) => {
            const active = section === n.href;
            const Icon = n.icon;
            return (
              <Link key={n.href} href={`/p/${pid}/${n.href}`} className={clsx("mx-2 my-0.5 flex items-center gap-2 rounded-md px-2 py-1.5 text-[13px]", active ? "bg-accent/15 text-accent" : "text-muted hover:bg-panel-2 hover:text-fg")}>
                <Icon size={15} /> {t(n.label)}
                {n.href === "issues" && openIssues > 0 && <span className="ml-auto rounded bg-err/20 px-1.5 text-[10px] text-err">{openIssues}</span>}
              </Link>
            );
          })}
          <Link href={`/p/${pid}/changelog`} className={clsx("mx-2 my-0.5 flex items-center gap-2 rounded-md px-2 py-1.5 text-[13px]", section === "changelog" ? "bg-accent/15 text-accent" : "text-muted hover:bg-panel-2 hover:text-fg")}>
            <Sparkles size={15} /> {t("What's new")}
            {unseen && <span className="ml-auto h-2 w-2 rounded-full bg-accent" title={LATEST_VERSION} />}
          </Link>
        </nav>
        <div className="border-t px-3 py-2 space-y-1.5 text-xs text-muted">
          <div className="flex items-center gap-1">
            <span className="flex-1">{t("Theme")}</span>
            {([["system", Monitor], ["dark", Moon], ["light", Sun]] as [Theme, typeof Sun][]).map(([v, Icon]) => (
              <button key={v} title={t(v === "system" ? "System" : v === "dark" ? "Dark" : "Light")} onClick={() => setTheme(v)} className={clsx("rounded p-1", theme === v ? "bg-accent/15 text-accent" : "hover:text-fg")}><Icon size={13} /></button>
            ))}
          </div>
          <div className="flex items-center gap-1">
            <span className="flex-1">{t("Language")}</span>
            <select value={locale} onChange={(e) => setLocale(e.target.value as Locale)} className="rounded border bg-panel px-1 py-0.5 text-[11px]" aria-label={t("Language")}>
              <option value="en">EN</option><option value="pt-BR">PT-BR</option>
            </select>
          </div>
          <div className="flex items-center justify-between">
            <span className="truncate">{me.data?.user.email}</span>
            <button title={t("Sign out")} onClick={async () => { await post("/api/auth/logout"); router.replace("/login"); }} className="hover:text-fg"><LogOut size={14} /></button>
          </div>
        </div>
      </aside>
      <main className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-11 items-center justify-between border-b bg-panel px-3 md:px-4 text-sm">
          <div className="flex items-center gap-2 min-w-0">
            <button className="md:hidden rounded p-1 text-muted hover:text-fg" onClick={() => setDrawer((d) => !d)} aria-label={t("Menu")}><Menu size={18} /></button>
            <div className="truncate text-muted">{project?.name ?? "…"} <span className="mx-1">/</span> <span className="text-fg">{t(sectionLabel)}</span></div>
          </div>
          <div className="flex items-center gap-2 text-muted">
            <button className="md:hidden rounded p-1 hover:text-fg" onClick={() => setPalette(true)} aria-label={t("Search")}><Search size={16} /></button>
            <button className="hidden md:inline rounded border px-1.5 text-[10px] hover:text-fg" onClick={() => setHelp(true)} title={t("Keyboard shortcuts")}>?</button>
            <span className="hidden md:inline text-[10px]" title={resolved}>{resolved === "light" ? <Sun size={12} /> : <Moon size={12} />}</span>
          </div>
        </header>
        <div className="page-pad flex-1 overflow-auto scroll-thin p-4">{children}</div>
        <AssistantDrawer />
        <CommandPalette pid={pid} open={palette} onClose={() => setPalette(false)} onAsk={openAssistant} />
        <ShortcutsSheet open={help} onClose={() => setHelp(false)} />
      </main>
    </div>
  );
}
