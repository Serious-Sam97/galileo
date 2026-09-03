"use client";

import { useEffect } from "react";
import { Markdown } from "@/components/assistant";
import { CHANGELOG, LATEST_VERSION } from "@/lib/changelog";
import { useT } from "@/lib/i18n";

export const SEEN_KEY = "galileo.changelog.seen";

export default function ChangelogPage() {
  const t = useT();
  useEffect(() => { try { localStorage.setItem(SEEN_KEY, LATEST_VERSION); window.dispatchEvent(new Event("galileo-changelog")); } catch {} }, []);
  return (
    <div className="mx-auto max-w-3xl">
      <h1 className="mb-1 text-lg font-semibold">{t("What's new")}</h1>
      <p className="mb-4 text-sm text-muted">Galileo {LATEST_VERSION}</p>
      <div className="prose-galileo rounded-xl border bg-panel p-5 text-sm"><Markdown>{CHANGELOG.replace(/^# Changelog\n+/, "")}</Markdown></div>
    </div>
  );
}
