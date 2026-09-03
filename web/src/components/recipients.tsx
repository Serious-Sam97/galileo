"use client";

import { Plus, X } from "lucide-react";
import { Input, Select } from "@/components/ui";
import { useProjectQuery } from "@/lib/hooks";
import type { Channel, RecipientRef, OncallSchedule } from "@/lib/types";

export type Recipient = RecipientRef;

/** Pick project channels (preferred) or paste a webhook/Slack URL inline. */
export function RecipientsEditor({ value, onChange }: { value: Recipient[]; onChange: (r: Recipient[]) => void }) {
  const channels = useProjectQuery<{ channels: Channel[] }>(["channels"], "/channels");
  const list = channels.data?.channels ?? [];
  const oncall = useProjectQuery<{ schedules: OncallSchedule[] }>(["oncall"], "/oncall");
  const schedules = oncall.data?.schedules ?? [];
  const setAt = (i: number, r: Recipient) => onChange(value.map((x, j) => (j === i ? r : x)));
  return (
    <div className="space-y-1.5">
      {value.map((r, i) => (
        <div key={i} className="flex gap-1.5">
          <Select value={r.type} onChange={(e) => { const t = e.target.value as Recipient["type"]; setAt(i, t === "channel" ? { type: "channel", id: list[0]?.id ?? "" } : t === "oncall" ? { type: "oncall", id: schedules[0]?.id ?? "" } : { type: t, url: "" }); }}>
            <option value="channel">Channel</option><option value="webhook">Webhook URL</option><option value="slack">Slack webhook URL</option><option value="oncall">On-call schedule</option>
          </Select>
          {r.type === "oncall" ? (
            <Select className="flex-1" value={r.id} onChange={(e) => setAt(i, { type: "oncall", id: e.target.value })}>
              {schedules.length === 0 && <option value="">no on-call schedules yet (Settings → Notifications)</option>}
              {schedules.map((sc) => <option key={sc.id} value={sc.id}>{sc.name}{sc.now ? ` · now: ${sc.now.email}` : ""}</option>)}
            </Select>
          ) : r.type === "channel" ? (
            <Select className="flex-1" value={r.id} onChange={(e) => setAt(i, { type: "channel", id: e.target.value })}>
              {list.length === 0 && <option value="">no channels yet (Settings → Notifications)</option>}
              {list.map((c) => <option key={c.id} value={c.id}>{c.name} · {c.kind}</option>)}
            </Select>
          ) : (
            <Input value={r.url} onChange={(e) => setAt(i, { type: r.type, url: e.target.value })} placeholder="https://…" className="font-mono" />
          )}
          <button type="button" className="text-muted hover:text-err" onClick={() => onChange(value.filter((_, j) => j !== i))}><X size={14} /></button>
        </div>
      ))}
      <button type="button" className="flex items-center gap-1 text-xs text-muted hover:text-fg" onClick={() => onChange([...value, list.length ? { type: "channel", id: list[0].id } : { type: "webhook", url: "" }])}><Plus size={12} /> recipient</button>
    </div>
  );
}
