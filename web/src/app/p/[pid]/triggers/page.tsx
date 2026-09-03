"use client";

import Link from "next/link";
import { useState } from "react";
import { useProjectId, useProjectQuery, useProjectMutation } from "@/lib/hooks";
import { post } from "@/lib/api";
import { Button, Table, Th, Td, Empty, Badge, Drawer, ErrorBox } from "@/components/ui";
import { TriggerForm, type TriggerDraft, emptyTrigger } from "./form";
import type { Trigger } from "@/lib/types";
import { ago, fmtDuration, fmtNum } from "@/lib/format";
import { Plus } from "lucide-react";

export default function TriggersPage() {
  const pid = useProjectId();
  const list = useProjectQuery<{ triggers: Trigger[] }>(["triggers"], "/triggers", { refetchInterval: 15_000 });
  const [creating, setCreating] = useState(false);
  const create = useProjectMutation<TriggerDraft>((p, b) => post(`/api/projects/${p}/triggers`, b), [["triggers"]]);
  const rows = list.data?.triggers ?? [];
  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <p className="text-muted text-sm">A trigger runs a query on a schedule and notifies when the result crosses a threshold. With breakdowns, the worst group decides.</p>
        <Button variant="primary" size="sm" onClick={() => setCreating(true)}><Plus size={13} /> Trigger</Button>
      </div>
      <ErrorBox error={list.error} />
      {rows.length === 0 ? <Empty>No triggers yet.</Empty> : (
        <Table>
          <thead><tr><Th>State</Th><Th>Name</Th><Th>Condition</Th><Th className="text-right">Last value</Th><Th>Every</Th><Th>Window</Th><Th>Evaluated</Th><Th>Last fired</Th></tr></thead>
          <tbody>
            {rows.map((t) => {
              const c = t.query.calculations[0];
              return (
                <tr key={t.id} className="hover:bg-panel-2">
                  <Td><Badge tone={t.state === "triggered" ? (t.severity === "warn" ? "warn" : "err") : t.state === "error" ? "warn" : t.state === "muted" ? "muted" : "ok"}>{t.state === "triggered" ? t.severity : t.state}</Badge>{!t.enabled && <Badge className="ml-1">off</Badge>}{t.mode === "baseline" && <Badge className="ml-1" tone="info">baseline</Badge>}{t.per_group && <Badge className="ml-1">per group</Badge>}</Td>
                  <Td><Link href={`/p/${pid}/triggers/${t.id}`} className="font-medium hover:text-accent">{t.name}</Link></Td>
                  <Td className="font-mono text-[11px]">{c ? `${c.op}${c.field ? `(${c.field})` : ""}` : "?"} {t.op} {t.mode === "baseline" ? `${t.baseline_factor}× last week` : t.threshold}{t.warn_threshold != null && t.mode === "threshold" ? ` (warn ${t.warn_threshold})` : ""}{t.query.breakdowns.length ? ` by ${t.query.breakdowns.join(",")}` : ""}{t.for_secs ? ` for ${Math.round(t.for_secs / 60)}m` : ""}</Td>
                  <Td className="text-right font-mono tabular-nums">{fmtNum(t.last_value)}</Td>
                  <Td className="text-muted">{fmtDuration(t.frequency_secs)}</Td>
                  <Td className="text-muted">{fmtDuration(t.window_secs)}</Td>
                  <Td className="text-muted">{ago(t.last_evaluated_at)}</Td>
                  <Td className="text-muted">{ago(t.last_triggered_at)}</Td>
                </tr>
              );
            })}
          </tbody>
        </Table>
      )}
      <Drawer open={creating} onClose={() => setCreating(false)} title="New trigger" width="w-[720px]">
        <TriggerForm initial={emptyTrigger()} error={create.error} onSubmit={async (d) => { await create.mutateAsync(d); setCreating(false); }} />
      </Drawer>
    </div>
  );
}
