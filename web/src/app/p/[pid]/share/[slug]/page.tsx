"use client";

import { use } from "react";
import Link from "next/link";
import { useProjectId, useProjectQuery, useRunQuery } from "@/lib/hooks";
import { SeriesCharts, HeatmapChart } from "@/components/query/result-charts";
import { GroupsTable, RawTable, ResultStats } from "@/components/query/results";
import { Empty, ErrorBox, Badge } from "@/components/ui";
import { fmtTime, encodeQ } from "@/lib/format";
import type { ShareDetail } from "@/lib/types";

export default function SharePage({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = use(params);
  const pid = useProjectId();
  const share = useProjectQuery<ShareDetail>(["share", slug], `/shares/${slug}`);
  const res = useRunQuery(share.data?.query ?? null);
  const s = share.data;
  return (
    <div className="space-y-3">
      <div className="rounded-lg border border-accent/40 bg-accent/5 p-3 text-sm flex flex-wrap items-center gap-2">
        <Badge tone="accent">shared view</Badge>
        {s && <span>{s.title || "Query"} · frozen to <b>{s.frozen_start ? fmtTime(s.frozen_start) : "?"}</b> → <b>{s.frozen_end ? fmtTime(s.frozen_end) : "?"}</b> · created {fmtTime(s.created_at)}</span>}
        {s && <Link href={`/p/${pid}/query?q=${encodeQ(s.query)}`} className="ml-auto text-info hover:underline">Open in Query builder →</Link>}
      </div>
      <ErrorBox error={share.error ?? res.error} />
      {res.data && (
        <>
          <ResultStats res={res.data} />
          {res.data.mode === "heatmap" && <HeatmapChart res={res.data} />}
          {res.data.groups.length > 0 && res.data.calculations.length > 0 && <SeriesCharts res={res.data} />}
          {res.data.mode === "series" && <GroupsTable res={res.data} />}
          {res.data.mode === "raw" && <RawTable res={res.data} />}
          {res.data.mode === "series" && res.data.groups.length === 0 && <Empty>No data in the frozen range.</Empty>}
        </>
      )}
    </div>
  );
}
