"use client";

import { useMe } from "@/lib/hooks";
import { Badge, Card, PageHeader } from "@/components/ui";
import { PasswordCard, SessionsCard, Tokens, TwoFactorCard } from "@/components/account";
import { useAccess } from "@/lib/hooks";

/** You: who you are here, your password, second factor, sessions and personal tokens. */
export default function AccountPage() {
  const me = useMe();
  const access = useAccess();
  const u = me.data?.user;
  return (
    <div className="mx-auto max-w-[1100px] space-y-4">
      <PageHeader title="Your account" sub="Password, two-factor authentication, where you are signed in, and tokens for scripts." />
      <Card>
        <div className="flex flex-wrap items-center gap-4">
          <span className="flex h-12 w-12 items-center justify-center rounded-full bg-gradient-to-br from-accent to-accent-2 text-lg font-bold text-accent-fg">{(u?.name || u?.email || "?").slice(0, 1).toUpperCase()}</span>
          <div className="min-w-0">
            <div className="text-[16px] font-semibold">{u?.name}</div>
            <div className="text-[13px] text-muted">{u?.email}</div>
          </div>
          <div className="ml-auto flex flex-wrap gap-1.5">
            {u?.is_master && <Badge tone="accent">Master</Badge>}
            {access.role && <Badge>{access.role} in this project</Badge>}
            {me.data?.orgs.map((o) => <Badge key={o.id}>{o.name}: {o.role}</Badge>)}
          </div>
        </div>
      </Card>
      <div className="grid gap-4 md:grid-cols-2">
        <PasswordCard />
        <div className="space-y-4">
          <TwoFactorCard />
          <SessionsCard />
        </div>
      </div>
      <Tokens />
    </div>
  );
}
