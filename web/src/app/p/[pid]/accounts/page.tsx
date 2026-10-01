"use client";

import { useMemo, useState } from "react";
import clsx from "clsx";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, Copy, KeyRound, Lock, Plus, ShieldCheck, Trash2, UserCog } from "lucide-react";
import { del, get, patch, post, put } from "@/lib/api";
import { useMe } from "@/lib/hooks";
import { ago, fmtTime } from "@/lib/format";
import { Badge, Button, Card, Drawer, Empty, ErrorBox, Input, Label, PageHeader, Select, Skeleton, Tabs } from "@/components/ui";

interface OrgRole { org_id: string; name: string; role: string }
interface ProjectRole { project_id: string; org_id: string; name: string; role: string }
interface Override { org_id: string; permission: string; allow: boolean }
interface Account {
  id: string; email: string; name: string; created_at: string; is_master: boolean; must_change_password: boolean;
  temp_password_expires_at: string | null; disabled_at: string | null; last_login_at: string | null; locked_until: string | null;
  totp_enabled: boolean; sso: boolean; sessions: number; orgs: OrgRole[]; projects: ProjectRole[]; overrides: Override[];
}
interface Catalog {
  permissions: { key: string; label: string; description: string }[];
  presets: Record<string, string[]>;
  org_roles: string[]; project_roles: string[];
  orgs: { id: string; name: string; projects: { id: string; name: string }[] }[];
  temp_password_hours: number;
}

const ROLE_HELP: Record<string, string> = {
  viewer: "Reads everything, changes nothing.",
  member: "Edits boards, queries, triggers and SLOs; triages issues; sees sensitive data.",
  admin: "Everything a member does, plus AI gateway, keys, settings, members and the audit log.",
  owner: "Everything, including creating and deleting projects.",
};

export default function AccountsPage() {
  const me = useMe();
  const users = useQuery({ queryKey: ["admin-users"], queryFn: () => get<{ users: Account[] }>("/api/admin/users"), enabled: !!me.data?.user.is_master });
  const catalog = useQuery({ queryKey: ["admin-catalog"], queryFn: () => get<Catalog>("/api/admin/catalog"), enabled: !!me.data?.user.is_master });
  const [creating, setCreating] = useState(false);
  const [managing, setManaging] = useState<string | null>(null);
  const [filter, setFilter] = useState("");

  if (me.data && !me.data.user.is_master) {
    return <div className="mx-auto max-w-[1100px]"><Empty>Accounts are managed by the Master of this Galileo.</Empty></div>;
  }
  const list = (users.data?.users ?? []).filter((u) => !filter || `${u.email} ${u.name}`.toLowerCase().includes(filter.toLowerCase()));
  const managed = users.data?.users.find((u) => u.id === managing) ?? null;

  return (
    <div className="mx-auto max-w-[1400px] space-y-4">
      <PageHeader
        title="Accounts"
        sub="Everyone who can sign in to this Galileo. You create accounts with a temporary password; people choose their own at first sign-in."
        actions={<>
          <Input className="w-56" placeholder="search name or e-mail" aria-label="Search accounts" value={filter} onChange={(e) => setFilter(e.target.value)} />
          <Button variant="primary" onClick={() => setCreating(true)}><Plus size={14} /> New account</Button>
        </>}
      />
      <ErrorBox error={users.error} />
      {users.isLoading ? <div className="space-y-2">{[0, 1, 2].map((i) => <Skeleton key={i} className="h-16" />)}</div> : (
        <div className="rise overflow-x-auto rounded-xl border bg-panel/80">
          <table className="w-full min-w-[860px] text-left text-[13px]">
            <thead>
              <tr className="text-[11px] uppercase tracking-[0.06em] text-faint">
                <th className="px-4 py-2.5 font-semibold">Account</th><th className="px-3 py-2.5 font-semibold">Organizations</th>
                <th className="px-3 py-2.5 font-semibold">Last sign-in</th><th className="px-3 py-2.5 font-semibold">Security</th><th className="px-3 py-2.5" />
              </tr>
            </thead>
            <tbody>
              {list.map((u) => (
                <tr key={u.id} className={clsx("border-t border-border/60 hover:bg-panel-2/60", u.disabled_at && "opacity-60")}>
                  <td className="px-4 py-3">
                    <div className="flex items-center gap-3">
                      <span className={clsx("flex h-9 w-9 shrink-0 items-center justify-center rounded-full font-semibold", u.is_master ? "bg-gradient-to-br from-accent to-accent-2 text-accent-fg" : "bg-panel-3 text-fg")}>{(u.name || u.email).slice(0, 1).toUpperCase()}</span>
                      <div className="min-w-0">
                        <div className="truncate font-medium">{u.name || u.email}{u.id === me.data?.user.id && <span className="ml-1.5 text-faint">(you)</span>}</div>
                        <div className="truncate text-[12px] text-muted">{u.email}</div>
                      </div>
                    </div>
                  </td>
                  <td className="px-3 py-3">
                    <div className="flex flex-wrap gap-1">
                      {u.is_master && <Badge tone="accent">Master</Badge>}
                      {u.orgs.map((o) => <Badge key={o.org_id}>{o.name}: {o.role}</Badge>)}
                      {!u.orgs.length && !u.is_master && <span className="text-faint">none</span>}
                    </div>
                  </td>
                  <td className="px-3 py-3 text-muted">{u.last_login_at ? ago(u.last_login_at) : "never"}</td>
                  <td className="px-3 py-3"><StatusBadges u={u} /></td>
                  <td className="px-3 py-3 text-right"><Button size="sm" onClick={() => setManaging(u.id)}><UserCog size={13} /> Manage</Button></td>
                </tr>
              ))}
            </tbody>
          </table>
          {list.length === 0 && <div className="p-6"><Empty>No account matches.</Empty></div>}
        </div>
      )}
      <Drawer open={creating} onClose={() => setCreating(false)} title="New account" width="w-[540px]">
        {catalog.data && <CreateAccount catalog={catalog.data} onDone={() => setCreating(false)} />}
      </Drawer>
      <Drawer open={!!managed} onClose={() => setManaging(null)} title={managed ? managed.name || managed.email : ""} width="w-[680px]">
        {managed && catalog.data && <ManageAccount u={managed} catalog={catalog.data} self={managed.id === me.data?.user.id} onDeleted={() => setManaging(null)} />}
      </Drawer>
    </div>
  );
}

function StatusBadges({ u }: { u: Account }) {
  const locked = u.locked_until && new Date(u.locked_until) > new Date();
  return (
    <div className="flex flex-wrap gap-1">
      {u.disabled_at ? <Badge tone="err">disabled</Badge> : locked ? <Badge tone="err">locked</Badge> : u.must_change_password ? (
        <Badge tone="warn" className="whitespace-nowrap">{u.temp_password_expires_at && new Date(u.temp_password_expires_at) < new Date() ? "temporary password expired" : "temporary password"}</Badge>
      ) : <Badge tone="ok">active</Badge>}
      {u.totp_enabled && <Badge tone="info">2FA</Badge>}
      {u.sso && <Badge>SSO</Badge>}
      {u.sessions > 0 && <Badge>{u.sessions} session{u.sessions === 1 ? "" : "s"}</Badge>}
    </div>
  );
}

/** The password shown once, with a copy button and what to do with it. */
function Reveal({ password, expires, email }: { password: string; expires: string; email: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <div className="rise space-y-3 rounded-xl border border-accent/40 bg-accent/[0.07] p-4">
      <div className="flex items-center gap-2 font-semibold"><KeyRound size={16} className="text-accent" /> Temporary password for {email}</div>
      <div className="flex items-center gap-2">
        <code className="flex-1 select-all rounded-lg border bg-bg px-3 py-2.5 font-mono text-[18px] tracking-wider">{password}</code>
        <Button onClick={async () => { await navigator.clipboard.writeText(password); setCopied(true); }} aria-label="Copy password">{copied ? <Check size={14} /> : <Copy size={14} />} {copied ? "Copied" : "Copy"}</Button>
      </div>
      <ul className="space-y-1 text-[12.5px] text-muted">
        <li>• Shown only now. Galileo keeps a hash, not the password.</li>
        <li>• Send it yourself, by a channel you trust. It works until {fmtTime(expires)}.</li>
        <li>• At their first sign-in they choose their own password; nothing else works until they do.</li>
      </ul>
    </div>
  );
}

function CreateAccount({ catalog, onDone }: { catalog: Catalog; onDone: () => void }) {
  const qc = useQueryClient();
  const [form, setForm] = useState({ email: "", name: "", org_id: catalog.orgs[0]?.id ?? "", role: "member" });
  const [projectRoles, setProjectRoles] = useState<Record<string, string>>({});
  const [result, setResult] = useState<{ temporary_password: string; expires_at: string } | null>(null);
  const org = catalog.orgs.find((o) => o.id === form.org_id);
  const create = useMutation({
    mutationFn: () => post<{ temporary_password: string; expires_at: string }>("/api/admin/users", {
      ...form, projects: Object.entries(projectRoles).filter(([, r]) => r).map(([project_id, role]) => ({ project_id, role })),
    }),
    onSuccess: (r) => { setResult(r); qc.invalidateQueries({ queryKey: ["admin-users"] }); },
  });
  if (result) {
    return (
      <div className="space-y-4">
        <Reveal password={result.temporary_password} expires={result.expires_at} email={form.email} />
        <Button variant="primary" onClick={onDone}>Done</Button>
      </div>
    );
  }
  return (
    <form className="space-y-4" onSubmit={(e) => { e.preventDefault(); create.mutate(); }}>
      <div className="grid gap-3 sm:grid-cols-2">
        <div><Label>E-mail</Label><Input type="email" required autoFocus value={form.email} onChange={(e) => setForm({ ...form, email: e.target.value })} /></div>
        <div><Label>Name</Label><Input value={form.name} placeholder="optional" onChange={(e) => setForm({ ...form, name: e.target.value })} /></div>
      </div>
      <div className="grid gap-3 sm:grid-cols-2">
        <div><Label>Organization</Label><Select className="w-full" value={form.org_id} onChange={(e) => { setForm({ ...form, org_id: e.target.value }); setProjectRoles({}); }}>{catalog.orgs.map((o) => <option key={o.id} value={o.id}>{o.name}</option>)}</Select></div>
        <div><Label>Role</Label><Select className="w-full" value={form.role} onChange={(e) => setForm({ ...form, role: e.target.value })}>{catalog.org_roles.map((r) => <option key={r} value={r}>{r}</option>)}</Select></div>
      </div>
      <p className="rounded-lg border bg-panel-2/60 px-3 py-2 text-[12.5px] text-muted">{ROLE_HELP[form.role]}</p>
      {org && org.projects.length > 0 && form.role !== "owner" && form.role !== "admin" && (
        <div>
          <Label>Project access (optional)</Label>
          <div className="space-y-1.5">
            {org.projects.map((p) => (
              <div key={p.id} className="flex items-center gap-2">
                <span className="flex-1 truncate">{p.name}</span>
                <Select value={projectRoles[p.id] ?? ""} onChange={(e) => setProjectRoles({ ...projectRoles, [p.id]: e.target.value })}>
                  <option value="">as the organization role</option>
                  {catalog.project_roles.map((r) => <option key={r} value={r}>{r}</option>)}
                </Select>
              </div>
            ))}
          </div>
        </div>
      )}
      <ErrorBox error={create.error} />
      <div className="flex items-center gap-3">
        <Button type="submit" variant="primary" disabled={create.isPending}><Plus size={14} /> Create with a temporary password</Button>
        <span className="text-[12px] text-faint">valid {catalog.temp_password_hours} h</span>
      </div>
    </form>
  );
}

function ManageAccount({ u, catalog, self, onDeleted }: { u: Account; catalog: Catalog; self: boolean; onDeleted: () => void }) {
  const [tab, setTab] = useState<"status" | "access" | "permissions">("status");
  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-2 text-[13px] text-muted">{u.email} · created {ago(u.created_at)} <StatusBadges u={u} /></div>
      <Tabs tabs={["status", "access", "permissions"] as const} value={tab} onChange={setTab} label={(t) => t === "access" ? "Organizations & projects" : t} />
      {tab === "status" && <StatusTab u={u} self={self} onDeleted={onDeleted} />}
      {tab === "access" && <AccessTab u={u} catalog={catalog} self={self} />}
      {tab === "permissions" && <PermissionsTab u={u} catalog={catalog} />}
    </div>
  );
}

function StatusTab({ u, self, onDeleted }: { u: Account; self: boolean; onDeleted: () => void }) {
  const qc = useQueryClient();
  const refresh = () => qc.invalidateQueries({ queryKey: ["admin-users"] });
  const [reset, setReset] = useState<{ temporary_password: string; expires_at: string } | null>(null);
  const update = useMutation({ mutationFn: (b: Record<string, unknown>) => patch(`/api/admin/users/${u.id}`, b), onSuccess: refresh });
  const resetPw = useMutation({ mutationFn: () => post<{ temporary_password: string; expires_at: string }>(`/api/admin/users/${u.id}/reset-password`), onSuccess: (r) => { setReset(r); refresh(); } });
  const remove = useMutation({ mutationFn: () => del(`/api/admin/users/${u.id}`), onSuccess: () => { refresh(); onDeleted(); } });
  const locked = u.locked_until && new Date(u.locked_until) > new Date();
  return (
    <div className="space-y-3">
      {reset && <Reveal password={reset.temporary_password} expires={reset.expires_at} email={u.email} />}
      <Action icon={KeyRound} title="Reset password" text="A new temporary password; they are signed out everywhere, their personal tokens are revoked, and they choose a new password at the next sign-in." disabled={self} hint={self ? "Change your own password from your account page." : undefined}>
        <Button onClick={() => confirm(`Reset the password of ${u.email}?`) && resetPw.mutate()} disabled={self || resetPw.isPending}>Reset</Button>
      </Action>
      {locked && (
        <Action icon={Lock} title="Locked after failed sign-ins" text={`Unlocks by itself ${ago(u.locked_until!)}; unlock now if it was them.`}>
          <Button onClick={() => update.mutate({ unlock: true })}>Unlock</Button>
        </Action>
      )}
      <Action icon={ShieldCheck} title={u.is_master ? "Master" : "Make Master"} text="The Master sees and manages every account, organization and project of this Galileo." disabled={self} hint={self ? "Another Master has to remove yours." : undefined}>
        <Button onClick={() => confirm(u.is_master ? `Remove Master from ${u.email}?` : `Make ${u.email} a Master? They will manage every account.`) && update.mutate({ is_master: !u.is_master })} disabled={self}>{u.is_master ? "Remove" : "Make Master"}</Button>
      </Action>
      <Action icon={Lock} title={u.disabled_at ? `Disabled ${ago(u.disabled_at)}` : "Disable"} text="A disabled account cannot sign in and is signed out everywhere. Its history stays." disabled={self}>
        <Button variant={u.disabled_at ? "outline" : "danger"} onClick={() => update.mutate({ disabled: !u.disabled_at })} disabled={self}>{u.disabled_at ? "Enable" : "Disable"}</Button>
      </Action>
      <Action icon={Trash2} title="Delete" text="Removes the account. Audit entries keep the e-mail. Prefer disabling if they might come back." disabled={self}>
        <Button variant="danger" onClick={() => confirm(`Delete ${u.email} for good?`) && remove.mutate()} disabled={self}>Delete</Button>
      </Action>
      <ErrorBox error={update.error ?? resetPw.error ?? remove.error} />
    </div>
  );
}

function Action({ icon: Icon, title, text, hint, disabled, children }: { icon: typeof Lock; title: string; text: string; hint?: string; disabled?: boolean; children: React.ReactNode }) {
  return (
    <div className={clsx("flex items-start gap-3 rounded-xl border bg-panel-2/40 p-3", disabled && "opacity-70")}>
      <Icon size={16} className="mt-0.5 shrink-0 text-lilac" />
      <div className="min-w-0 flex-1">
        <div className="font-medium">{title}</div>
        <div className="text-[12.5px] text-muted">{hint ?? text}</div>
      </div>
      {children}
    </div>
  );
}

function AccessTab({ u, catalog, self }: { u: Account; catalog: Catalog; self: boolean }) {
  const qc = useQueryClient();
  const refresh = () => qc.invalidateQueries({ queryKey: ["admin-users"] });
  const setOrg = useMutation({ mutationFn: (b: { org: string; role: string | null }) => put(`/api/admin/users/${u.id}/orgs/${b.org}`, { role: b.role }), onSuccess: refresh });
  const setProject = useMutation({ mutationFn: (b: { project: string; role: string | null }) => put(`/api/admin/users/${u.id}/projects/${b.project}`, { role: b.role }), onSuccess: refresh });
  return (
    <div className="space-y-3">
      {catalog.orgs.map((o) => {
        const member = u.orgs.find((m) => m.org_id === o.id);
        return (
          <Card key={o.id} title={o.name} actions={
            <Select value={member?.role ?? ""} aria-label={`Role in ${o.name}`} onChange={(e) => (e.target.value || confirm(`Remove ${u.email} from ${o.name}?`)) && setOrg.mutate({ org: o.id, role: e.target.value || null })}>
              <option value="">not a member</option>
              {catalog.org_roles.map((r) => <option key={r} value={r}>{r}</option>)}
            </Select>
          }>
            {!member ? <p className="text-[12.5px] text-faint">Pick a role to add {self ? "yourself" : "them"} to this organization.</p> : (
              <div className="space-y-1.5">
                <p className="text-[12.5px] text-muted">{ROLE_HELP[member.role]}</p>
                {member.role !== "owner" && member.role !== "admin" && o.projects.map((p) => {
                  const pr = u.projects.find((x) => x.project_id === p.id);
                  return (
                    <div key={p.id} className="flex items-center gap-2">
                      <span className="flex-1 truncate">{p.name}</span>
                      <Select value={pr?.role ?? ""} aria-label={`Role in ${p.name}`} onChange={(e) => setProject.mutate({ project: p.id, role: e.target.value || null })}>
                        <option value="">as the organization role</option>
                        {catalog.project_roles.map((r) => <option key={r} value={r}>{r}</option>)}
                      </Select>
                    </div>
                  );
                })}
              </div>
            )}
          </Card>
        );
      })}
      <ErrorBox error={setOrg.error ?? setProject.error} />
    </div>
  );
}

function PermissionsTab({ u, catalog }: { u: Account; catalog: Catalog }) {
  const qc = useQueryClient();
  const [orgId, setOrgId] = useState(u.orgs[0]?.org_id ?? "");
  const member = u.orgs.find((o) => o.org_id === orgId);
  const preset = useMemo(() => new Set(member ? catalog.presets[member.role] ?? [] : []), [member, catalog.presets]);
  const saved = useMemo(() => Object.fromEntries(u.overrides.filter((o) => o.org_id === orgId).map((o) => [o.permission, o.allow])), [u.overrides, orgId]);
  const [draft, setDraft] = useState<Record<string, boolean | null>>({});
  const value = (k: string): boolean | null => (k in draft ? draft[k] : k in saved ? saved[k] : null);
  const save = useMutation({
    mutationFn: () => put(`/api/admin/users/${u.id}/permissions`, { org_id: orgId, overrides: draft }),
    onSuccess: () => { setDraft({}); qc.invalidateQueries({ queryKey: ["admin-users"] }); },
  });
  if (u.is_master) return <Empty>A Master holds every permission everywhere.</Empty>;
  if (!u.orgs.length) return <Empty>Add the account to an organization first.</Empty>;
  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <Label>Organization</Label>
        <Select value={orgId} onChange={(e) => { setOrgId(e.target.value); setDraft({}); }}>{u.orgs.map((o) => <option key={o.org_id} value={o.org_id}>{o.name} ({o.role})</option>)}</Select>
      </div>
      <p className="text-[12.5px] text-muted">The role <b className="text-fg">{member?.role}</b> gives the ticked permissions. Grant or remove single ones for this person; seeing the project&apos;s data needs no permission.</p>
      <div className="overflow-hidden rounded-xl border">
        {catalog.permissions.map((p) => {
          const v = value(p.key);
          const effective = v ?? preset.has(p.key);
          return (
            <div key={p.key} className={clsx("flex items-center gap-3 border-b border-border/60 px-3 py-2.5 last:border-0", v !== null && "bg-accent/[0.06]")}>
              <span className={clsx("flex h-5 w-5 shrink-0 items-center justify-center rounded-md text-[11px]", effective ? "bg-ok/20 text-ok" : "bg-panel-3 text-faint")} aria-label={effective ? "allowed" : "not allowed"}>{effective ? "✓" : "–"}</span>
              <div className="min-w-0 flex-1">
                <div className="text-[13px] font-medium">{p.label}</div>
                <div className="text-[11.5px] text-faint">{p.description}</div>
              </div>
              <div className="flex shrink-0 gap-0.5 rounded-lg border bg-bg/60 p-0.5" role="radiogroup" aria-label={p.label}>
                {([[null, `Role (${preset.has(p.key) ? "yes" : "no"})`], [true, "Grant"], [false, "Remove"]] as const).map(([val, label]) => (
                  <button key={String(val)} type="button" role="radio" aria-checked={v === val} onClick={() => setDraft({ ...draft, [p.key]: val })}
                    className={clsx("rounded-md px-2 py-1 text-[11.5px]", v === val ? (val === true ? "bg-ok/20 text-ok" : val === false ? "bg-err/20 text-err" : "bg-panel-3 text-fg") : "text-muted hover:text-fg")}>{label}</button>
                ))}
              </div>
            </div>
          );
        })}
      </div>
      <ErrorBox error={save.error} />
      <div className="flex gap-2">
        <Button variant="primary" disabled={!Object.keys(draft).length || save.isPending} onClick={() => save.mutate()}>Save permissions</Button>
        {Object.keys(draft).length > 0 && <Button variant="ghost" onClick={() => setDraft({})}>Discard</Button>}
      </div>
    </div>
  );
}
