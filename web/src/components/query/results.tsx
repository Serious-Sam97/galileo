"use client";

import Link from "next/link";
import { useState } from "react";
import clsx from "clsx";
import { Table, Th, Td, Empty, Badge } from "@/components/ui";
import { useProjectId } from "@/lib/hooks";
import type { QueryResponse } from "@/lib/types";
import { colorFor, fmtMs, fmtNum, fmtTime } from "@/lib/format";
import { groupLabel } from "./result-charts";

export function GroupsTable({ res }: { res: QueryResponse }) {
  if (!res.groups.length) return <Empty>No results in this time range.</Empty>;
  return (
    <Table className="max-h-[420px]">
      <thead><tr><Th></Th>{res.breakdowns.map((b) => <Th key={b}>{b}</Th>)}{res.calculations.map((c) => <Th key={c} className="text-right">{c}</Th>)}</tr></thead>
      <tbody>
        {res.groups.map((g, gi) => (
          <tr key={gi} className="hover:bg-panel-2">
            <Td className="w-6"><span className="inline-block h-2.5 w-2.5 rounded-sm" style={{ background: colorFor(gi) }} /></Td>
            {g.key.map((k, i) => <Td key={i} className="font-mono">{k || <span className="text-muted">∅</span>}</Td>)}
            {g.totals.map((t, i) => <Td key={i} className="text-right tabular-nums font-mono">{fmtNum(t)}</Td>)}
          </tr>
        ))}
      </tbody>
    </Table>
  );
}

const cellClass = (col: string) => (col === "timestamp" ? "whitespace-nowrap text-muted" : col === "body" ? "max-w-[520px]" : "");

function Cell({ col, v }: { col: string; v: unknown }) {
  const pid = useProjectId();
  if (v === null || v === undefined || v === "") return <span className="text-muted">–</span>;
  if (col === "timestamp") return <>{fmtTime(String(v))}</>;
  if (col === "trace_id") return <Link className="font-mono text-info hover:underline" href={`/p/${pid}/traces/${v}`}>{String(v).slice(0, 8)}…</Link>;
  if (col === "duration_ms") return <span className="font-mono tabular-nums">{fmtMs(Number(v))}</span>;
  if (col === "status_code" || col === "severity") {
    const s = String(v);
    const tone = s === "error" || s === "fatal" ? "err" : s === "warn" ? "warn" : s === "ok" || s === "info" ? "ok" : "muted";
    return <Badge tone={tone}>{s}</Badge>;
  }
  if (col === "attrs" && typeof v === "object") {
    const entries = Object.entries(v as Record<string, string>);
    return <span className="text-muted text-[11px] font-mono">{entries.slice(0, 6).map(([k, x]) => `${k}=${x}`).join("  ")}{entries.length > 6 ? ` +${entries.length - 6}` : ""}</span>;
  }
  if (typeof v === "number") return <span className="font-mono tabular-nums">{fmtNum(v)}</span>;
  return <span className={clsx(col === "body" && "whitespace-pre-wrap break-words")}>{String(v)}</span>;
}

export function RawTable({ res, onRowClick }: { res: QueryResponse; onRowClick?: (row: Record<string, unknown>) => void }) {
  const raw = res.raw!;
  const [hidden] = useState(new Set(["parent_span_id", "span_id"]));
  const cols = raw.columns.map((c, i) => ({ c, i })).filter(({ c }) => !hidden.has(c));
  if (!raw.rows.length) return <Empty>No events match.</Empty>;
  return (
    <Table className="max-h-[600px]">
      <thead><tr>{cols.map(({ c }) => <Th key={c}>{c}</Th>)}</tr></thead>
      <tbody>
        {raw.rows.map((r, ri) => (
          <tr key={ri} className={clsx("hover:bg-panel-2", onRowClick && "cursor-pointer")} onClick={() => onRowClick?.(Object.fromEntries(raw.columns.map((c, i) => [c, r[i]])))}>
            {cols.map(({ c, i }) => <Td key={c} className={cellClass(c)}><Cell col={c} v={r[i]} /></Td>)}
          </tr>
        ))}
      </tbody>
    </Table>
  );
}

export function ResultStats({ res }: { res: QueryResponse }) {
  return (
    <div className="flex items-center gap-3 text-[11px] text-muted">
      <span>{res.mode}</span><span>·</span>
      <span>{(res.stats.elapsed * 1000).toFixed(0)}ms</span><span>·</span>
      <span>{fmtNum(res.stats.rows_read)} rows read</span>
      {res.mode !== "raw" && <><span>·</span><span>{res.granularity}s buckets</span></>}
      {res.raw && <><span>·</span><span>{res.raw.rows.length} rows</span></>}
      {res.groups.length > 0 && res.mode === "series" && <><span>·</span><span>{res.groups.length} groups</span></>}
    </div>
  );
}
