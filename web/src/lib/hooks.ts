"use client";

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useParams } from "next/navigation";
import { get, post } from "./api";
import type { Me, Query, QueryResponse, FieldInfo, Dataset } from "./types";

export function useMe() {
  return useQuery({ queryKey: ["me"], queryFn: () => get<Me>("/api/auth/me"), retry: false });
}

export function useProjectId(): string {
  const p = useParams<{ pid: string }>();
  return p.pid;
}

export function useProjectQuery<T>(key: unknown[], path: string, opts: { enabled?: boolean; refetchInterval?: number } = {}) {
  const pid = useProjectId();
  return useQuery({
    queryKey: [pid, ...key],
    queryFn: () => get<T>(`/api/projects/${pid}${path}`),
    ...opts,
  });
}

export function useRunQuery(q: Query | null, opts: { enabled?: boolean; refetchInterval?: number } = {}) {
  const pid = useProjectId();
  return useQuery({
    queryKey: [pid, "query", q],
    queryFn: () => post<QueryResponse>(`/api/projects/${pid}/query`, q),
    enabled: !!q && (opts.enabled ?? true),
    refetchInterval: opts.refetchInterval,
    placeholderData: (prev) => prev,
  });
}

export function useFields(dataset: Dataset) {
  const pid = useProjectId();
  return useQuery({
    queryKey: [pid, "fields", dataset],
    queryFn: () => get<{ fields: FieldInfo[] }>(`/api/projects/${pid}/fields?dataset=${dataset}`),
    staleTime: 60_000,
  });
}

export function useInvalidate() {
  const qc = useQueryClient();
  const pid = useProjectId();
  return (...key: unknown[]) => qc.invalidateQueries({ queryKey: [pid, ...key] });
}

export function useProjectMutation<TIn, TOut = unknown>(fn: (pid: string, input: TIn) => Promise<TOut>, invalidate: unknown[][] = []) {
  const pid = useProjectId();
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (input: TIn) => fn(pid, input),
    onSuccess: () => invalidate.forEach((k) => qc.invalidateQueries({ queryKey: [pid, ...k] })),
  });
}

export type PermKey = "view_sensitive" | "edit_content" | "triage_issues" | "manage_ai" | "manage_ingest" | "manage_project" | "manage_members" | "audit_export" | "manage_org";

/** What the signed-in user may do in the current project. The server enforces it; the UI uses it to
 *  hide what would only answer "forbidden". */
export function useAccess() {
  const q = useProjectQuery<{ role: string; permissions: PermKey[]; is_master: boolean }>(["access"], "", { refetchInterval: 5 * 60_000 });
  const perms = new Set(q.data?.permissions ?? []);
  return { loaded: !!q.data, role: q.data?.role, isMaster: !!q.data?.is_master, can: (p: PermKey) => perms.has(p) };
}
