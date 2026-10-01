// Server-side PNG render of a board (or one query) for sharing: ECharts SSR → SVG → PNG (resvg).
// GET /api/render?pid=…&board=…  |  GET /api/render?pid=…&q=<encoded query>&title=…
// The signed-in user's cookie is forwarded to the Galileo API, so access rules apply.
import { NextRequest } from "next/server";
import * as echarts from "echarts";
import { Resvg } from "@resvg/resvg-js";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { C, SERIES } from "@/lib/palette";

const FONT = "DejaVu Sans";
let fontFile: string | undefined;
try { fontFile = join(process.cwd(), "public/fonts/DejaVuSans.ttf"); readFileSync(fontFile); } catch { fontFile = undefined; }

const API = process.env.GALILEO_API_INTERNAL ?? process.env.NEXT_PUBLIC_GALILEO_API ?? "http://localhost:8080";
const PALETTE = SERIES;

async function api<T>(path: string, cookie: string, init?: RequestInit): Promise<T> {
  const r = await fetch(`${API}${path}`, { ...init, headers: { cookie, "content-type": "application/json", ...(init?.headers ?? {}) }, cache: "no-store" });
  if (!r.ok) throw new Error(`${path}: ${r.status}`);
  return r.json();
}

type Res = { mode: string; breakdowns: string[]; calculations: string[]; groups: { key: string[]; totals: (number | null)[]; series: { ts: number; values: (number | null)[] }[] }[] };

function chartSvg(title: string, res: Res, w: number, h: number): string {
  const chart = echarts.init(null, null, { renderer: "svg", ssr: true, width: w, height: h });
  const shown = res.groups.slice(0, 8);
  const series = shown.flatMap((g, gi) => res.calculations.slice(0, 3).map((c, ci) => ({
    name: `${g.key.length ? g.key.join(",") : c}${res.calculations.length > 1 ? ` · ${c}` : ""}`, type: "line", showSymbol: false, lineStyle: { width: 1.5, color: PALETTE[(gi * 3 + ci) % PALETTE.length] }, itemStyle: { color: PALETTE[(gi * 3 + ci) % PALETTE.length] },
    data: g.series.map((p) => [p.ts * 1000, p.values[ci]]),
  })));
  chart.setOption({
    backgroundColor: C.bg, animation: false,
    title: { text: title, left: 10, top: 6, textStyle: { color: C.fg, fontSize: 13, fontWeight: 500, fontFamily: FONT } },
    textStyle: { fontFamily: FONT },
    legend: { bottom: 4, textStyle: { color: C.faint, fontSize: 10, fontFamily: FONT }, type: "scroll" },
    grid: { left: 48, right: 16, top: 34, bottom: 40 },
    xAxis: { type: "time", axisLabel: { color: C.faint, fontSize: 10, fontFamily: FONT }, axisLine: { lineStyle: { color: C.border } } },
    yAxis: { type: "value", axisLabel: { color: C.faint, fontSize: 10, fontFamily: FONT }, splitLine: { lineStyle: { color: C.grid } } },
    series,
  });
  const svg = chart.renderToSVGString();
  chart.dispose();
  return svg;
}

function statSvg(title: string, value: string, sub: string, w: number, h: number): string {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}"><rect width="100%" height="100%" fill="${C.bg}"/><text x="12" y="22" fill="${C.fg}" font-size="13" font-family="DejaVu Sans,Helvetica,Arial">${esc(title)}</text><text x="12" y="${h / 2 + 12}" fill="#f5a524" font-size="36" font-weight="600" font-family="DejaVu Sans,Helvetica,Arial">${esc(value)}</text><text x="12" y="${h - 14}" fill="${C.faint}" font-size="11" font-family="DejaVu Sans,Helvetica,Arial">${esc(sub)}</text></svg>`;
}
const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
const fmt = (v: number | null | undefined) => v == null ? "–" : Math.abs(v) >= 1000 ? `${(v / 1000).toFixed(1)}k` : v.toFixed(Math.abs(v) < 10 ? 2 : 0);

/** Substitute $var in filters/breakdowns with the board's values (mirrors the client). */
function substitute(q: Record<string, unknown>, vars: Record<string, string>): Record<string, unknown> {
  const s = JSON.stringify(q).replace(/\$([a-zA-Z_][a-zA-Z0-9_]*)/g, (m, n) => (n in vars ? vars[n] : m));
  const out = JSON.parse(s) as Record<string, unknown>;
  out.filters = ((out.filters as { value?: unknown }[]) ?? []).filter((f) => !(typeof f.value === "string" && (f.value === "" || f.value.startsWith("$"))));
  return out;
}

export async function GET(req: NextRequest) {
  const cookie = req.headers.get("cookie") ?? "";
  const u = req.nextUrl.searchParams;
  const pid = u.get("pid"); if (!pid) return new Response("pid required", { status: 400 });
  const W = 1200, PW = 600, PH = 300;
  const tiles: { svg: string; x: number; y: number; w: number; h: number }[] = [];
  let title = "Galileo";
  try {
    if (u.get("board")) {
      const { board } = await api<{ board: { name: string; panels: Record<string, unknown>[]; variables?: { name: string; default?: string }[]; time_range?: unknown } }>(`/api/projects/${pid}/boards/${u.get("board")}`, cookie);
      title = board.name;
      const vars: Record<string, string> = {}; (board.variables ?? []).forEach((v) => { vars[v.name] = u.get(`v_${v.name}`) ?? v.default ?? ""; });
      const panels = board.panels.filter((p) => ["line", "table", "stat", undefined].includes(p.viz as string | undefined)).slice(0, 8);
      for (let i = 0; i < panels.length; i++) {
        const p = panels[i];
        let q = substitute(p.query as Record<string, unknown>, vars);
        if (board.time_range) q = { ...q, time_range: board.time_range };
        const res = await api<Res>(`/api/projects/${pid}/query`, cookie, { method: "POST", body: JSON.stringify(q) });
        const x = (i % 2) * PW, y = Math.floor(i / 2) * PH;
        const svg = p.viz === "stat" ? statSvg(String(p.title), fmt(res.groups[0]?.totals[0]), String(res.calculations[0] ?? ""), PW, PH) : chartSvg(String(p.title), res, PW, PH);
        tiles.push({ svg, x, y, w: PW, h: PH });
      }
    } else if (u.get("q")) {
      const q = JSON.parse(Buffer.from(u.get("q")!.replace(/-/g, "+").replace(/_/g, "/"), "base64").toString());
      title = u.get("title") ?? "Query";
      const res = await api<Res>(`/api/projects/${pid}/query`, cookie, { method: "POST", body: JSON.stringify(q) });
      tiles.push({ svg: chartSvg(title, res, W, 480), x: 0, y: 0, w: W, h: 480 });
    } else return new Response("board or q required", { status: 400 });
  } catch (e) { return new Response(`render failed: ${(e as Error).message}`, { status: 502 }); }
  const H = Math.max(...tiles.map((t) => t.y + t.h)) + 28;
  const inner = tiles.map((t) => `<g transform="translate(${t.x},${t.y + 28})">${t.svg.replace(/<\?xml[^>]*>/, "").replace(/<svg([^>]*)>/, `<svg$1>`)}</g>`).join("");
  const composed = `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}"><rect width="100%" height="100%" fill="${C.bg}"/><text x="10" y="19" fill="${C.fg}" font-size="14" font-weight="600" font-family="DejaVu Sans,Helvetica,Arial">${esc(title)} · ${esc(new Date().toISOString().slice(0, 16).replace("T", " "))} UTC</text>${inner}</svg>`;
  const png = new Resvg(composed, { fitTo: { mode: "width", value: W }, font: { loadSystemFonts: !fontFile, fontFiles: fontFile ? [fontFile] : [], defaultFontFamily: FONT } }).render().asPng();
  return new Response(new Uint8Array(png), { headers: { "content-type": "image/png", "cache-control": "no-store", "content-disposition": `inline; filename="${title.replace(/[^a-z0-9-]+/gi, "-").toLowerCase()}.png"` } });
}
