import { AsyncLocalStorage } from "node:async_hooks";

/** Who is acting, propagated through async context so DB spans and logs can carry it. */
export interface Identity { id?: string | number; email?: string; name?: string; tenant?: string | number; [k: string]: unknown }

const als = new AsyncLocalStorage<Identity>();

export function setIdentity(identity: Identity): void {
  const cur = als.getStore();
  if (cur) Object.assign(cur, identity);
  else als.enterWith({ ...identity });
}
export function currentIdentity(): Identity | undefined { return als.getStore(); }
export function runWithIdentity<T>(identity: Identity, fn: () => T): T { return als.run({ ...identity }, fn); }

export function identityAttributes(id: Identity | undefined): Record<string, string | number> {
  if (!id) return {};
  const out: Record<string, string | number> = {};
  if (id.id !== undefined) out["user.id"] = String(id.id);
  if (id.email) out["user.email"] = String(id.email);
  if (id.name) out["user.name"] = String(id.name);
  if (id.tenant !== undefined) out["tenant.id"] = String(id.tenant);
  return out;
}
