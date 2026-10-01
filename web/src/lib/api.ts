export const API_BASE = process.env.NEXT_PUBLIC_GALILEO_API ?? "http://localhost:8080";

/** OTLP/HTTP base that apps export to. Locally the receiver has its own port (:8080 → :4318);
 *  behind the reverse proxy it lives under the API host at `/otlp` (see deploy/Caddyfile).
 *  NEXT_PUBLIC_GALILEO_OTLP overrides both. */
export const OTLP_BASE = (process.env.NEXT_PUBLIC_GALILEO_OTLP ?? otlpFrom(API_BASE)).replace(/\/$/, "");

export function otlpFrom(api: string): string {
  const base = api.replace(/\/$/, "");
  return /:8080$/.test(base) ? base.replace(/:8080$/, ":4318") : `${base}/otlp`;
}

export class ApiError extends Error {
  status: number;
  code: string;
  constructor(status: number, code: string, message: string) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

export async function api<T = unknown>(path: string, init: RequestInit & { json?: unknown } = {}): Promise<T> {
  const { json, headers, ...rest } = init;
  const res = await fetch(`${API_BASE}${path}`, {
    credentials: "include",
    ...rest,
    headers: { ...(json !== undefined ? { "content-type": "application/json" } : {}), ...(headers ?? {}) },
    body: json !== undefined ? JSON.stringify(json) : rest.body,
  });
  const text = await res.text();
  let body: unknown = null;
  try {
    body = text ? JSON.parse(text) : null;
  } catch {
    body = text;
  }
  if (!res.ok) {
    const err = (body as { error?: { code?: string; message?: string } } | null)?.error;
    throw new ApiError(res.status, err?.code ?? "error", err?.message ?? `${res.status} ${res.statusText}`);
  }
  return body as T;
}

export const get = <T,>(path: string) => api<T>(path);
export const post = <T,>(path: string, json?: unknown) => api<T>(path, { method: "POST", json });
export const put = <T,>(path: string, json?: unknown) => api<T>(path, { method: "PUT", json });
export const patch = <T,>(path: string, json?: unknown) => api<T>(path, { method: "PATCH", json });
export const del = <T,>(path: string) => api<T>(path, { method: "DELETE" });
