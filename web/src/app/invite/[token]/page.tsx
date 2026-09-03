"use client";

import { useParams, useRouter } from "next/navigation";
import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { get, post } from "@/lib/api";
import { Button, ErrorBox, Input, Label } from "@/components/ui";
import { Telescope } from "lucide-react";

export default function InvitePage() {
  const { token } = useParams<{ token: string }>();
  const router = useRouter();
  const qc = useQueryClient();
  const info = useQuery({ queryKey: ["invite", token], queryFn: () => get<{ org: string; email: string; role: string; user_exists: boolean }>(`/api/auth/invite/${token}`), retry: false });
  const [name, setName] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
  async function accept(e: React.FormEvent) {
    e.preventDefault(); setBusy(true); setError(null);
    try {
      const r = await post<{ project_id: string | null; org_id: string }>(`/api/auth/invite/${token}/accept`, { name, password });
      qc.clear();
      router.replace(r.project_id ? `/p/${r.project_id}/overview` : `/org/${r.org_id}`);
    } catch (err) { setError(err); } finally { setBusy(false); }
  }
  return (
    <div className="flex flex-1 items-center justify-center p-6">
      <form onSubmit={accept} className="w-full max-w-sm rounded-xl border bg-panel p-6 space-y-4">
        <div className="flex items-center gap-2 text-lg font-semibold"><Telescope className="text-accent" size={22} /> Galileo</div>
        {info.error ? <ErrorBox error={info.error} /> : info.data ? (
          <>
            <p className="text-sm">You were invited to <b>{info.data.org}</b> as <b>{info.data.role}</b>, for <span className="font-mono">{info.data.email}</span>.</p>
            {info.data.user_exists ? <p className="text-muted text-xs">You already have an account with this e-mail. Enter its password to join.</p> : (
              <div><Label>Your name</Label><Input value={name} onChange={(e) => setName(e.target.value)} placeholder="Ana" /></div>
            )}
            <div><Label>{info.data.user_exists ? "Password" : "Choose a password"}</Label><Input type="password" required minLength={8} value={password} onChange={(e) => setPassword(e.target.value)} /></div>
            <ErrorBox error={error} />
            <Button type="submit" variant="primary" className="w-full justify-center" disabled={busy}>Join {info.data.org}</Button>
          </>
        ) : <p className="text-muted text-sm">Checking invite…</p>}
      </form>
    </div>
  );
}
