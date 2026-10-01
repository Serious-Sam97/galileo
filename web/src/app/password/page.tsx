"use client";

import { useEffect, useState } from "react";
import { useRouter } from "next/navigation";
import { useQueryClient } from "@tanstack/react-query";
import { post } from "@/lib/api";
import { useMe } from "@/lib/hooks";
import { Button, Input, Label, ErrorBox } from "@/components/ui";
import { AuthShell } from "@/components/auth-shell";
import { PasswordHints } from "@/components/account";

/** After signing in with a temporary password: choose your own before anything else. */
export default function ChoosePasswordPage() {
  const router = useRouter();
  const qc = useQueryClient();
  const me = useMe();
  const [form, setForm] = useState({ next: "", again: "" });
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (me.isError) router.replace("/login");
    else if (me.data && !me.data.user.must_change_password) router.replace("/");
  }, [me.data, me.isError, router]);

  const mismatch = form.again !== "" && form.again !== form.next;

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (mismatch) return;
    setBusy(true);
    setError(null);
    try {
      await post("/api/auth/password", { new_password: form.next });
      qc.clear();
      router.replace("/");
    } catch (err) {
      setError(err);
    } finally {
      setBusy(false);
    }
  }

  return (
    <AuthShell>
      <form onSubmit={submit} className="space-y-4">
        <div>
          <p className="text-[15px] font-semibold">Choose your password</p>
          <p className="mt-1 text-[13px] text-muted">
            {me.data ? <>Welcome, {me.data.user.name || me.data.user.email}. </> : null}
            You signed in with a temporary password. Pick one only you know to continue.
          </p>
        </div>
        <div><Label>New password</Label><Input type="password" autoComplete="new-password" autoFocus required minLength={10} value={form.next} onChange={(e) => setForm({ ...form, next: e.target.value })} /></div>
        <div><Label>New password, again</Label><Input type="password" autoComplete="new-password" required value={form.again} onChange={(e) => setForm({ ...form, again: e.target.value })} /></div>
        <PasswordHints value={form.next} />
        {mismatch && <p className="text-[12.5px] text-err">The two passwords differ.</p>}
        <ErrorBox error={error} />
        <Button type="submit" variant="primary" className="w-full justify-center" disabled={busy || mismatch}>Save and continue</Button>
        <button type="button" className="w-full text-center text-xs text-muted hover:text-fg" onClick={async () => { await post("/api/auth/logout"); qc.clear(); router.replace("/login"); }}>
          Sign out
        </button>
      </form>
    </AuthShell>
  );
}
