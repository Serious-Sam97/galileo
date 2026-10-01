// The vapor palette: the one place colors are defined for code that cannot use CSS variables
// (ECharts options, the server-side PNG renderer). globals.css mirrors these values as --tokens.

export const C = {
  bg: "#0d0818",
  panel: "#140c27",
  panel2: "#1c1236",
  panel3: "#241845",
  border: "#2a1f47",
  grid: "#1e1538",
  fg: "#ece6ff",
  muted: "#a99fcc",
  faint: "#8a80b4",
  accent: "#ff5ccf", // magenta: primary actions, active state
  violet: "#8b5cff",
  lilac: "#b9a6ff",
  cyan: "#5ee9ff",
  ok: "#4be3a3",
  warn: "#ffc35c",
  err: "#ff6b95",
} as const;

/** Series colors, in order. Neighbours differ in lightness as well as hue. */
export const SERIES = ["#ff5ccf", "#5ee9ff", "#b9a6ff", "#ffc35c", "#4be3a3", "#ff8a6b", "#7aa7ff", "#ffb3e6", "#c6f36b", "#9d93c4"];

export const colorFor = (i: number) => SERIES[i % SERIES.length];

/** ECharts tooltip box in the palette. */
export const tooltipStyle = {
  backgroundColor: C.panel2,
  borderColor: C.border,
  textStyle: { color: C.fg, fontSize: 11 },
  extraCssText: "border-radius: 10px; box-shadow: 0 8px 28px rgba(0,0,0,.45);",
};
