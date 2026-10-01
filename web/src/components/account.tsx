"use client";

import { useState } from "react";
import clsx from "clsx";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { Copy, LogOut, Monitor, Trash2 } from "lucide-react";
import { Button, Card, Table, Th, Td, Empty, Badge, ErrorBox, Input, Label } from "@/components/ui";
import { del as apiDel, get as apiGet, post as apiPost } from "@/lib/api";
import { ago, fmtTime } from "@/lib/format";
import type { ApiToken } from "@/lib/types";

/** Change your own password: the current one, then the new one twice. */
export function PasswordCard() {
  const [form, setForm] = useState({ current: "", next: "", again: "" });
  const [done, setDone] = useState<number | null>(null);
  const change = useMutation({
    mutationFn: () => apiPost<{ signed_out_sessions: number }>("/api/auth/password", { current_password: form.current, new_password: form.next }),
    onSuccess: (r) => { setDone(r.signed_out_sessions); setForm({ current: "", next: "", again: "" }); },
  });
  const mismatch = form.again !== "" && form.again !== form.next;
  return (
    <Card title="Password">
      <form className="space-y-3" onSubmit={(e) => { e.preventDefault(); if (!mismatch) change.mutate(); }}>
        <div><Label>Current password</Label><Input type="password" autoComplete="current-password" required value={form.current} onChange={(e) => setForm({ ...form, current: e.target.value })} /></div>
        <div><Label>New password</Label><Input type="password" autoComplete="new-password" required minLength={10} value={form.next} onChange={(e) => setForm({ ...form, next: e.target.value })} /></div>
        <div><Label>New password, again</Label><Input type="password" autoComplete="new-password" required value={form.again} onChange={(e) => setForm({ ...form, again: e.target.value })} /></div>
        <PasswordHints value={form.next} />
        {mismatch && <p className="text-[12.5px] text-err">The two new passwords differ.</p>}
        <ErrorBox error={change.error} />
        {done != null && <p className="text-[12.5px] text-ok">Password changed.{done > 0 ? ` ${done} other session${done === 1 ? " was" : "s were"} signed out.` : ""}</p>}
        <Button type="submit" variant="primary" disabled={change.isPending || mismatch}>Change password</Button>
      </form>
    </Card>
  );
}

/** The password rule as a checklist that ticks while typing. */
export function PasswordHints({ value }: { value: string }) {
  const rules = [
    ["At least 10 characters", value.length >= 10],
    ["Not one character repeated", value.length > 0 && !value.split("").every((c) => c === value[0])],
  ] as const;
  return (
    <ul className="space-y-1 text-[12.5px]">
      {rules.map(([label, ok]) => <li key={label} className={ok ? "text-ok" : "text-faint"}>{ok ? "✓" : "○"} {label}</li>)}
      <li className="text-faint">○ Not containing your e-mail name (checked on save)</li>
    </ul>
  );
}

interface Session { id: string; created_at: string; expires_at: string; user_agent: string; current: boolean }

/** Where you are signed in, with one-click sign-out of a device or of all the others. */
export function SessionsCard() {
  const qc = useQueryClient();
  const q = useQuery({ queryKey: ["sessions"], queryFn: () => apiGet<{ sessions: Session[] }>("/api/auth/sessions") });
  const revoke = useMutation({ mutationFn: (id: string) => apiDel(`/api/auth/sessions/${id}`), onSuccess: () => qc.invalidateQueries({ queryKey: ["sessions"] }) });
  const others = (q.data?.sessions ?? []).filter((s) => !s.current).length;
  return (
    <Card title="Where you are signed in" actions={others > 0 && <Button size="sm" variant="danger" onClick={() => revoke.mutate("others")}><LogOut size={12} /> Sign out the other {others}</Button>}>
      {q.data?.sessions.length === 0 ? <Empty>No sessions.</Empty> : (
        <ul className="divide-y divide-border/60">
          {q.data?.sessions.map((s) => (
            <li key={s.id} className="flex items-center gap-3 py-2.5">
              <Monitor size={16} className={s.current ? "text-cyan" : "text-faint"} />
              <div className="min-w-0 flex-1">
                <div className="truncate text-[13px]">{browserName(s.user_agent)}{s.current && <Badge tone="info" className="ml-2">this device</Badge>}</div>
                <div className="text-[11.5px] text-faint">signed in {ago(s.created_at)} · until {fmtTime(s.expires_at)}</div>
              </div>
              {!s.current && <Button size="sm" variant="ghost" onClick={() => revoke.mutate(s.id)} aria-label="Sign out this session"><LogOut size={12} /></Button>}
            </li>
          ))}
        </ul>
      )}
      <ErrorBox error={revoke.error} />
    </Card>
  );
}

function browserName(ua: string): string {
  if (!ua) return "Unknown device";
  const os = /Windows/.test(ua) ? "Windows" : /Mac OS X/.test(ua) ? "macOS" : /Android/.test(ua) ? "Android" : /iPhone|iPad/.test(ua) ? "iOS" : /Linux/.test(ua) ? "Linux" : "";
  const br = /Edg\//.test(ua) ? "Edge" : /Firefox\//.test(ua) ? "Firefox" : /Chrome\//.test(ua) ? "Chrome" : /Safari\//.test(ua) ? "Safari" : /curl|python|Go-http/i.test(ua) ? "Script" : "Browser";
  return os ? `${br} on ${os}` : br;
}

export function Tokens() {
  const qc = useQueryClient();
  const list = useQuery({ queryKey: ["tokens"], queryFn: () => apiGet<{ tokens: ApiToken[] }>("/api/auth/tokens") });
  const [name, setName] = useState("");
  const [days, setDays] = useState("");
  const [created, setCreated] = useState<string | null>(null);
  const create = useMutation({ mutationFn: () => apiPost<{ token: string }>("/api/auth/tokens", { name, expires_days: days ? Number(days) : null }), onSuccess: (r) => { setCreated(r.token); setName(""); qc.invalidateQueries({ queryKey: ["tokens"] }); } });
  const revoke = useMutation({ mutationFn: (id: string) => apiDel(`/api/auth/tokens/${id}`), onSuccess: () => qc.invalidateQueries({ queryKey: ["tokens"] }) });
  return (
    <div className="grid gap-3 md:grid-cols-2">
      <Card title="Personal API tokens">
        <p className="text-muted text-sm mb-3">For scripts and CI calling the Galileo API as you: <span className="font-mono">Authorization: Bearer glt_…</span>. Same permissions as your account.</p>
        <form className="flex gap-2" onSubmit={(e) => { e.preventDefault(); create.mutate(); }}><Input required placeholder="token name (e.g. ci deploy markers)" value={name} onChange={(e) => setName(e.target.value)} /><Input className="w-28" type="number" placeholder="days" value={days} onChange={(e) => setDays(e.target.value)} /><Button type="submit" variant="primary">Create</Button></form>
        <ErrorBox error={create.error} />
        {created && <div className="mt-3 rounded border bg-bg p-2 text-xs"><div className="text-muted mb-1">Copy it now, it is shown once.</div><div className="flex gap-2"><Input readOnly value={created} className="font-mono" /><Button onClick={() => navigator.clipboard.writeText(created)}><Copy size={13} /></Button></div></div>}
      </Card>
      <Card title="Your tokens">
        {list.data?.tokens.length === 0 ? <Empty>No tokens.</Empty> : (
          <Table><thead><tr><Th>Name</Th><Th>Prefix</Th><Th>Last used</Th><Th>Expires</Th><Th></Th></tr></thead>
            <tbody>{list.data?.tokens.map((t) => <tr key={t.id} className={clsx(t.revoked_at && "opacity-50")}><Td>{t.name}</Td><Td className="font-mono">{t.token_prefix}…</Td><Td className="text-muted">{ago(t.last_used_at)}</Td><Td className="text-muted">{t.expires_at ? fmtTime(t.expires_at) : "never"}</Td><Td className="text-right">{t.revoked_at ? <Badge tone="err">revoked</Badge> : <Button size="sm" variant="ghost" onClick={() => revoke.mutate(t.id)}><Trash2 size={12} /></Button>}</Td></tr>)}</tbody></Table>
        )}
      </Card>
    </div>
  );
}

export function TwoFactorCard() {
  const q = useQuery({ queryKey: ["2fa"], queryFn: () => apiGet<{ enabled: boolean }>("/api/auth/2fa") });
  const [setup, setSetup] = useState<{ secret: string; otpauth_url: string } | null>(null);
  const [code, setCode] = useState("");
  const qc = useQueryClient();
  const start = useMutation({ mutationFn: () => apiPost<{ secret: string; otpauth_url: string }>("/api/auth/2fa/setup"), onSuccess: (r) => setSetup(r) });
  const enable = useMutation({ mutationFn: () => apiPost("/api/auth/2fa/enable", { code }), onSuccess: () => { setSetup(null); setCode(""); qc.invalidateQueries({ queryKey: ["2fa"] }); } });
  const disable = useMutation({ mutationFn: () => apiPost("/api/auth/2fa/disable", { code }), onSuccess: () => { setCode(""); qc.invalidateQueries({ queryKey: ["2fa"] }); } });
  return (
    <Card title="Two-factor authentication">
      <p className="text-muted text-sm mb-2">Adds a TOTP code (Google Authenticator, 1Password, Aegis…) to password logins. SSO logins are governed by your identity provider.</p>
      {q.data?.enabled ? (
        <div className="space-y-2">
          <Badge tone="ok">enabled</Badge>
          <div className="flex gap-2 items-end"><div><Label>Code to disable</Label><Input value={code} onChange={(e) => setCode(e.target.value)} placeholder="123456" /></div><Button onClick={() => disable.mutate()}>Disable</Button></div>
          <ErrorBox error={disable.error} />
        </div>
      ) : setup ? (
        <div className="space-y-2">
          <p className="text-sm">Add this secret to your authenticator app, then enter the current code.</p>
          <pre className="rounded border bg-bg p-2 font-mono text-[11px] whitespace-pre-wrap break-all">{setup.secret}</pre>
          <p className="text-[11px] text-muted break-all">{setup.otpauth_url}</p>
          <div className="flex gap-2 items-end"><div><Label>Code</Label><Input value={code} onChange={(e) => setCode(e.target.value)} placeholder="123456" /></div><Button variant="primary" onClick={() => enable.mutate()}>Enable</Button></div>
          <ErrorBox error={enable.error} />
        </div>
      ) : (
        <Button onClick={() => start.mutate()}>Set up 2FA</Button>
      )}
    </Card>
  );
}
