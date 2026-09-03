"use client";

import Link from "next/link";
import { useState } from "react";
import { useProjectId, useProjectQuery, useProjectMutation } from "@/lib/hooks";
import { post } from "@/lib/api";
import { Button, Table, Th, Td, Empty, Badge, Drawer, ErrorBox } from "@/components/ui";
import { SloForm, emptySlo, type SloDraft } from "./form";
import type { Slo } from "@/lib/types";
import { ago } from "@/lib/format";
import { Plus } from "lucide-react";

const tone = (s: string) => (s === "burning" || s === "exhausted" ? "err" : s === "error" ? "warn" : "ok");

export default function SlosPage() {
  const pid = useProjectId();
  const list = useProjectQuery<{ slos: Slo[] }>(["slos"], "/slos", { refetchInterval: 30_000 });
  const [creating, setCreating] = useState(false);
  const create = useProjectMutation<SloDraft>((p, b) => post(`/api/projects/${p}/slos`, b), [["slos"]]);
  const rows = list.data?.slos ?? [];
  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <p className="text-muted text-sm">An SLO tracks the share of good events over a rolling window against a target, and alerts when the error budget burns too fast.</p>
        <Button variant="primary" size="sm" onClick={() => setCreating(true)}><Plus size={13} /> SLO</Button>
      </div>
      <ErrorBox error={list.error} />
      {rows.length === 0 ? <Empty>No SLOs yet.</Empty> : (
        <Table>
          <thead><tr><Th>State</Th><Th>Name</Th><Th className="text-right">SLI</Th><Th className="text-right">Target</Th><Th className="text-right">Budget left</Th><Th>Window</Th><Th>Evaluated</Th></tr></thead>
          <tbody>
            {rows.map((s) => {
              const r = s.last_result;
              return (
                <tr key={s.id} className="hover:bg-panel-2">
                  <Td><Badge tone={tone(s.state)}>{s.state}</Badge></Td>
                  <Td><Link href={`/p/${pid}/slos/${s.id}`} className="font-medium hover:text-accent">{s.name}</Link></Td>
                  <Td className="text-right font-mono tabular-nums">{r?.sli_pct != null ? r.sli_pct.toFixed(3) + "%" : "–"}</Td>
                  <Td className="text-right font-mono tabular-nums">{s.target_pct}%</Td>
                  <Td className="text-right font-mono tabular-nums">{r?.budget_remaining_pct != null ? <span className={r.budget_remaining_pct < 0 ? "text-err" : r.budget_remaining_pct < 25 ? "text-warn" : ""}>{r.budget_remaining_pct.toFixed(1)}%</span> : "–"}</Td>
                  <Td className="text-muted">{s.window_days}d</Td>
                  <Td className="text-muted">{ago(s.last_evaluated_at)}</Td>
                </tr>
              );
            })}
          </tbody>
        </Table>
      )}
      <Drawer open={creating} onClose={() => setCreating(false)} title="New SLO" width="w-[720px]">
        <SloForm initial={emptySlo()} error={create.error} onSubmit={async (d) => { await create.mutateAsync(d); setCreating(false); }} />
      </Drawer>
    </div>
  );
}
