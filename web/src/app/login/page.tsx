"use client";

import { useState } from "react";
import { useRouter } from "next/navigation";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { get, post } from "@/lib/api";
import { Button, Input, Label, ErrorBox } from "@/components/ui";
import { Telescope } from "lucide-react";

export default function LoginPage() {
  const router = useRouter();
  const qc = useQueryClient();
  const setup = useQuery({ queryKey: ["setup"], queryFn: () => get<{ needs_setup: boolean }>("/api/auth/setup") });
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
        ? await post<{ project?: { id: string } }>("/api/auth/2fa/verify", { email: form.email, password: form.password, code })
        : await post<{ project?: { id: string }; needs_2fa?: boolean }>(`/api/auth/${effective}`, form);
      if ("needs_2fa" in res && res.needs_2fa) { setNeeds2fa(true); return; }
      qc.clear();
      if (res.project) router.replace(`/p/${res.project.id}/overview`);
      else router.replace("/");
    } catch (err) {
      setError(err);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex flex-1 items-center justify-center p-6">
      <form onSubmit={submit} className="w-full max-w-sm rounded-xl border bg-panel p-6 space-y-4">
        <div className="flex items-center gap-2 text-lg font-semibold"><Telescope className="text-accent" size={22} /> Galileo</div>
        <p className="text-muted text-sm">
          {effective === "register" ? (setup.data?.needs_setup ? "Create the first account for this instance." : "Create an account.") : "Sign in to your workspace."}
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
        <ErrorBox error={error} />
        <Button type="submit" variant="primary" className="w-full justify-center" disabled={busy}>
          {effective === "register" ? "Create account" : "Sign in"}
        </Button>
        {sso.data?.enabled && effective === "login" && (
          <a href="/api/auth/oidc/start" className="block w-full rounded-md border px-3 py-2 text-center text-sm hover:bg-panel-2">{sso.data.label || "Continue with SSO"}</a>
        )}
        <button type="button" className="w-full text-center text-xs text-muted hover:text-fg" onClick={() => setMode(effective === "login" ? "register" : "login")}>
          {effective === "login" ? "Need an account? Register" : "Have an account? Sign in"}
        </button>
      </form>
    </div>
  );
}
