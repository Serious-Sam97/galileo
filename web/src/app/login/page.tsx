"use client";

import { Suspense, useState } from "react";
import { useRouter, useSearchParams } from "next/navigation";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ApiError, get, post } from "@/lib/api";
import { Button, Input, Label, ErrorBox } from "@/components/ui";
import { AuthShell } from "@/components/auth-shell";

export default function LoginPage() {
  return <Suspense><LoginForm /></Suspense>;
}

function LoginForm() {
  const router = useRouter();
  const qc = useQueryClient();
  const params = useSearchParams();
  const setup = useQuery({ queryKey: ["setup"], queryFn: () => get<{ needs_setup: boolean; registration_open?: boolean }>("/api/auth/setup") });
  const [mode, setMode] = useState<"login" | "register" | null>(null);
  const [form, setForm] = useState({ email: "", password: "", name: "", org_name: "" });
  const [code, setCode] = useState("");
  const [needs2fa, setNeeds2fa] = useState(false);
  const sso = useQuery({ queryKey: ["oidc"], queryFn: () => get<{ enabled: boolean; label: string }>("/api/auth/oidc") });
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
  const effective = mode ?? (setup.data?.needs_setup ? "register" : "login");

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const res = needs2fa
        ? await post<{ project?: { id: string }; must_change_password?: boolean }>("/api/auth/2fa/verify", { email: form.email, password: form.password, code })
        : await post<{ project?: { id: string }; needs_2fa?: boolean; must_change_password?: boolean }>(`/api/auth/${effective}`, form);
      if ("needs_2fa" in res && res.needs_2fa) { setNeeds2fa(true); return; }
      qc.clear();
      if (res.must_change_password) router.replace("/password");
      else if (res.project) router.replace(`/p/${res.project.id}/overview`);
      else router.replace("/");
    } catch (err) {
      // A wrong e-mail and a wrong password answer the same, on purpose.
      setError(err instanceof ApiError && err.code === "unauthorized" ? new Error(needs2fa ? "That code is not right." : "Wrong e-mail or password.") : err);
    } finally {
      setBusy(false);
    }
  }

  return (
    <AuthShell>
      <form onSubmit={submit} className="space-y-4">
        <p className="text-sm font-medium">
          {effective === "register" ? "Create the first account. It becomes the Master of this Galileo." : "Sign in to your workspace."}
        </p>
        {effective === "register" && (
          <>
            <div><Label>Your name</Label><Input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} placeholder="Sam" /></div>
            <div><Label>Organization</Label><Input value={form.org_name} onChange={(e) => setForm({ ...form, org_name: e.target.value })} placeholder="Acme" /></div>
          </>
        )}
        <div><Label>Email</Label><Input type="email" required value={form.email} onChange={(e) => setForm({ ...form, email: e.target.value })} /></div>
        <div><Label>Password</Label><Input type="password" required minLength={8} value={form.password} onChange={(e) => setForm({ ...form, password: e.target.value })} /></div>
        {needs2fa && (
          <div><Label>Authenticator code</Label><Input autoFocus inputMode="numeric" pattern="[0-9]*" required value={code} onChange={(e) => setCode(e.target.value)} placeholder="123456" /></div>
        )}
        {params.get("sso") === "no_account" && !error && <div className="rounded-lg border border-warn/40 bg-warn/10 px-3 py-2 text-[13px] text-warn">There is no Galileo account for that sign-in. Ask the Master to create one.</div>}
        <ErrorBox error={error} />
        <Button type="submit" variant="primary" className="w-full justify-center" disabled={busy}>
          {effective === "register" ? "Create account" : "Sign in"}
        </Button>
        {sso.data?.enabled && effective === "login" && (
          <a href="/api/auth/oidc/start" className="block w-full rounded-lg border px-3 py-2 text-center text-sm hover:bg-panel-2">{sso.data.label || "Continue with SSO"}</a>
        )}
        {setup.data?.registration_open && (
          <button type="button" className="w-full text-center text-xs text-muted hover:text-fg" onClick={() => setMode(effective === "login" ? "register" : "login")}>
            {effective === "login" ? "First time here? Create the first account" : "Have an account? Sign in"}
          </button>
        )}
        {!setup.data?.registration_open && <p className="text-center text-[11.5px] text-faint">Accounts are created by the Master of this Galileo.</p>}
      </form>
    </AuthShell>
  );
}
