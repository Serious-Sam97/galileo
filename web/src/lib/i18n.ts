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
  Accounts: { en: "Accounts", "pt-BR": "Contas" },
  "Your account": { en: "Your account", "pt-BR": "Sua conta" },
  Watch: { en: "Watch", "pt-BR": "Acompanhar" },
  Explore: { en: "Explore", "pt-BR": "Explorar" },
  Product: { en: "Product", "pt-BR": "Produto" },
  Build: { en: "Build", "pt-BR": "Construir" },
  Main: { en: "Main", "pt-BR": "Principal" },
  Project: { en: "Project", "pt-BR": "Projeto" },
  Live: { en: "Live", "pt-BR": "Ao vivo" },
  "Pages refresh on their own": { en: "Pages refresh on their own", "pt-BR": "As páginas se atualizam sozinhas" },
  "Time range": { en: "Time range", "pt-BR": "Período" },
  "Last 15 min": { en: "Last 15 min", "pt-BR": "Últimos 15 min" },
  "Last hour": { en: "Last hour", "pt-BR": "Última hora" },
  "Last 4 hours": { en: "Last 4 hours", "pt-BR": "Últimas 4 horas" },
  "Last 24 hours": { en: "Last 24 hours", "pt-BR": "Últimas 24 horas" },
  "Last 3 days": { en: "Last 3 days", "pt-BR": "Últimos 3 dias" },
  "Last 7 days": { en: "Last 7 days", "pt-BR": "Últimos 7 dias" },
  "Last 30 days": { en: "Last 30 days", "pt-BR": "Últimos 30 dias" },
  "Needs attention": { en: "Needs attention", "pt-BR": "Pedem atenção" },
  "All clear": { en: "All clear", "pt-BR": "Tudo certo" },
  "Slowest routes": { en: "Slowest routes", "pt-BR": "Rotas mais lentas" },
  "Open in Query": { en: "Open in Query", "pt-BR": "Abrir na Consulta" },
  "p95 latency": { en: "p95 latency", "pt-BR": "Latência p95" },
  Errors: { en: "Errors", "pt-BR": "Erros" },
  "Open issues": { en: "Open issues", "pt-BR": "Problemas abertos" },
  "LLM spend": { en: "LLM spend", "pt-BR": "Gasto com LLM" },
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
