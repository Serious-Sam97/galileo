"use client";

import Link from "next/link";
import { useState } from "react";
import { useProjectId, useProjectQuery, useProjectMutation } from "@/lib/hooks";
import { post, del } from "@/lib/api";
import { Button, Table, Th, Td, Empty, Drawer, ErrorBox, Input, Label, Textarea } from "@/components/ui";
import type { Board, SavedQuery } from "@/lib/types";
import { ago, encodeQ } from "@/lib/format";
import { Plus, Trash2 } from "lucide-react";

export default function BoardsPage() {
  const pid = useProjectId();
  const boards = useProjectQuery<{ boards: Board[] }>(["boards"], "/boards");
  const saved = useProjectQuery<{ saved_queries: SavedQuery[] }>(["saved-queries"], "/saved-queries");
  const [creating, setCreating] = useState(false);
  const [form, setForm] = useState({ name: "", description: "" });
  const create = useProjectMutation<typeof form>((p, b) => post(`/api/projects/${p}/boards`, { ...b, panels: [] }), [["boards"]]);
  const fromTemplate = useProjectMutation<{ kind: string; service?: string }>((p, b) => post(`/api/projects/${p}/boards/templates`, b), [["boards"]]);
  const services = useProjectQuery<{ services: { service_name: string }[] }>(["services-24h"], "/services?last_seconds=86400");
  const [tpl, setTpl] = useState({ kind: "red", service: "" });
  const removeBoard = useProjectMutation<string>((p, id) => del(`/api/projects/${p}/boards/${id}`), [["boards"]]);
  const removeQuery = useProjectMutation<string>((p, id) => del(`/api/projects/${p}/saved-queries/${id}`), [["saved-queries"]]);
  return (
    <div className="grid gap-4 md:grid-cols-2">
      <div className="space-y-3">
        <div className="flex items-center justify-between"><h2 className="font-semibold">Boards</h2><div className="flex items-center gap-2"><select className="rounded-md border bg-bg px-2 py-1 text-xs" value={tpl.kind} onChange={(e) => setTpl({ ...tpl, kind: e.target.value })}><option value="red">RED per service</option><option value="llm">LLM cost &amp; quality</option><option value="browser">Browser vitals</option></select>{tpl.kind === "red" && <select className="rounded-md border bg-bg px-2 py-1 text-xs" value={tpl.service} onChange={(e) => setTpl({ ...tpl, service: e.target.value })}><option value="">all services</option>{services.data?.services.map((s) => <option key={s.service_name} value={s.service_name}>{s.service_name}</option>)}</select>}<Button size="sm" onClick={() => fromTemplate.mutate({ kind: tpl.kind, service: tpl.service || undefined })}>New from template</Button><Button variant="primary" size="sm" onClick={() => setCreating(true)}><Plus size={13} /> Board</Button></div></div>
        <ErrorBox error={boards.error} />
        {boards.data?.boards.length === 0 ? <Empty>No boards. Save queries, then pin them to a board.</Empty> : (
          <Table><thead><tr><Th>Name</Th><Th className="text-right">Panels</Th><Th>Updated</Th><Th></Th></tr></thead>
            <tbody>{boards.data?.boards.map((b) => <tr key={b.id} className="hover:bg-panel-2"><Td><Link href={`/p/${pid}/boards/${b.id}`} className="font-medium hover:text-accent">{b.name}</Link><div className="text-muted text-[11px]">{b.description}</div></Td><Td className="text-right">{b.panels.length}</Td><Td className="text-muted">{ago(b.updated_at)}</Td><Td className="text-right"><Button size="sm" variant="ghost" onClick={() => confirm("Delete board?") && removeBoard.mutate(b.id)}><Trash2 size={12} /></Button></Td></tr>)}</tbody></Table>
        )}
      </div>
      <div className="space-y-3">
        <h2 className="font-semibold">Saved queries</h2>
        {saved.data?.saved_queries.length === 0 ? <Empty>Nothing saved yet. Use “Save” on the Query page.</Empty> : (
          <Table><thead><tr><Th>Name</Th><Th>Dataset</Th><Th>Updated</Th><Th></Th></tr></thead>
            <tbody>{saved.data?.saved_queries.map((s) => <tr key={s.id} className="hover:bg-panel-2"><Td><Link href={`/p/${pid}/query?q=${encodeQ(s.query)}`} className="font-medium hover:text-accent">{s.name}</Link><div className="text-muted text-[11px]">{s.description}</div></Td><Td className="text-muted">{s.query.dataset}</Td><Td className="text-muted">{ago(s.updated_at)}</Td><Td className="text-right"><Button size="sm" variant="ghost" onClick={() => confirm("Delete saved query?") && removeQuery.mutate(s.id)}><Trash2 size={12} /></Button></Td></tr>)}</tbody></Table>
        )}
      </div>
      <Drawer open={creating} onClose={() => setCreating(false)} title="New board">
        <form className="space-y-3" onSubmit={async (e) => { e.preventDefault(); await create.mutateAsync(form); setCreating(false); }}>
          <div><Label>Name</Label><Input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} required /></div>
          <div><Label>Description</Label><Textarea rows={3} value={form.description} onChange={(e) => setForm({ ...form, description: e.target.value })} /></div>
          <ErrorBox error={create.error} />
          <Button type="submit" variant="primary">Create</Button>
        </form>
      </Drawer>
    </div>
  );
}
