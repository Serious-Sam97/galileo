"use client";

import { useCallback, useEffect, useState } from "react";

export type Locale = "en" | "pt-BR";

const DICT: Record<string, Record<Locale, string>> = {
  Overview: { en: "Overview", "pt-BR": "Visão geral" },
  Query: { en: "Query", "pt-BR": "Consulta" },
  Traces: { en: "Traces", "pt-BR": "Traces" },
  Logs: { en: "Logs", "pt-BR": "Logs" },
  Issues: { en: "Issues", "pt-BR": "Problemas" },
  Browser: { en: "Browser", "pt-BR": "Navegador" },
  Map: { en: "Map", "pt-BR": "Mapa" },
  Metrics: { en: "Metrics", "pt-BR": "Métricas" },
  AI: { en: "AI", "pt-BR": "IA" },
  Boards: { en: "Boards", "pt-BR": "Painéis" },
  Triggers: { en: "Triggers", "pt-BR": "Alertas" },
  SLOs: { en: "SLOs", "pt-BR": "SLOs" },
  Settings: { en: "Settings", "pt-BR": "Configurações" },
  "What's new": { en: "What's new", "pt-BR": "Novidades" },
  "All projects": { en: "All projects", "pt-BR": "Todos os projetos" },
  "Sign out": { en: "Sign out", "pt-BR": "Sair" },
  Search: { en: "Search", "pt-BR": "Buscar" },
  "Search or jump to…": { en: "Search issues, boards, routes, users, or paste a trace id…", "pt-BR": "Busque problemas, painéis, rotas, usuários ou cole um trace id…" },
  Theme: { en: "Theme", "pt-BR": "Tema" },
  System: { en: "System", "pt-BR": "Sistema" },
  Dark: { en: "Dark", "pt-BR": "Escuro" },
  Light: { en: "Light", "pt-BR": "Claro" },
  Language: { en: "Language", "pt-BR": "Idioma" },
  Save: { en: "Save", "pt-BR": "Salvar" },
  Cancel: { en: "Cancel", "pt-BR": "Cancelar" },
  Delete: { en: "Delete", "pt-BR": "Excluir" },
  Create: { en: "Create", "pt-BR": "Criar" },
  Run: { en: "Run", "pt-BR": "Executar" },
  "Last 1h": { en: "Last 1h", "pt-BR": "Última hora" },
  "No data": { en: "No data", "pt-BR": "Sem dados" },
  "No traces yet.": { en: "No traces yet.", "pt-BR": "Nenhum trace ainda." },
  "Keyboard shortcuts": { en: "Keyboard shortcuts", "pt-BR": "Atalhos de teclado" },
  "Go to": { en: "Go to", "pt-BR": "Ir para" },
  Actions: { en: "Actions", "pt-BR": "Ações" },
  "Recent queries": { en: "Recent queries", "pt-BR": "Consultas recentes" },
  Users: { en: "Users", "pt-BR": "Usuários" },
  Routes: { en: "Routes", "pt-BR": "Rotas" },
  Prompts: { en: "Prompts", "pt-BR": "Prompts" },
  "Open trace": { en: "Open trace", "pt-BR": "Abrir trace" },
  "New trigger": { en: "New trigger", "pt-BR": "Novo alerta" },
  "New board": { en: "New board", "pt-BR": "Novo painel" },
  "Ask Galileo": { en: "Ask Galileo", "pt-BR": "Perguntar ao Galileo" },
  "Compare with…": { en: "Compare with…", "pt-BR": "Comparar com…" },
  "Get started": { en: "Get started", "pt-BR": "Começar" },
  Requests: { en: "Requests", "pt-BR": "Requisições" },
  "Error rate": { en: "Error rate", "pt-BR": "Taxa de erros" },
  Services: { en: "Services", "pt-BR": "Serviços" },
  Menu: { en: "Menu", "pt-BR": "Menu" },
};

const KEY = "galileo.locale";
let current: Locale = "en";
const listeners = new Set<() => void>();

export function getLocale(): Locale {
  try { const v = localStorage.getItem(KEY); if (v === "pt-BR" || v === "en") return v; } catch {}
  return "en";
}
export function setLocale(l: Locale) {
  current = l;
  try { localStorage.setItem(KEY, l); } catch {}
  if (typeof document !== "undefined") document.documentElement.lang = l === "pt-BR" ? "pt-BR" : "en";
  listeners.forEach((f) => f());
}
export function t(key: string, locale: Locale = current): string { return DICT[key]?.[locale] ?? key; }

/** Translate hook: `const t = useT(); t("Overview")`. Re-renders on locale change. */
export function useT() {
  const [, force] = useState(0);
  useEffect(() => { current = getLocale(); force((x) => x + 1); const f = () => force((x) => x + 1); listeners.add(f); return () => { listeners.delete(f); }; }, []);
  return useCallback((key: string) => t(key, current), []);
}
export function useLocale(): [Locale, (l: Locale) => void] {
  const [, force] = useState(0);
  useEffect(() => { current = getLocale(); force((x) => x + 1); const f = () => force((x) => x + 1); listeners.add(f); return () => { listeners.delete(f); }; }, []);
  return [current, setLocale];
}
export function localeTag(): string { return current === "pt-BR" ? "pt-BR" : "en-US"; }
