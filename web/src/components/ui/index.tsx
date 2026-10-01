"use client";

import clsx from "clsx";
import { X } from "lucide-react";
import { useEffect, useId } from "react";
import { Logo } from "@/components/logo";
import { C } from "@/lib/palette";

type BtnProps = React.ButtonHTMLAttributes<HTMLButtonElement> & { variant?: "primary" | "ghost" | "danger" | "outline"; size?: "sm" | "md" };
export function Button({ variant = "outline", size = "md", className, ...p }: BtnProps) {
  return (
    <button
      {...p}
      className={clsx(
        "inline-flex items-center gap-1.5 rounded-lg font-medium transition-[background,border-color,box-shadow,filter] disabled:opacity-50 disabled:cursor-not-allowed whitespace-nowrap",
        size === "sm" ? "px-2.5 py-1 text-xs" : "px-3.5 py-1.5 text-[13px] min-h-[34px]",
        variant === "primary" && "btn-vapor font-semibold",
        variant === "outline" && "border bg-panel-2/70 hover:bg-panel-3 hover:border-accent/40",
        variant === "ghost" && "hover:bg-panel-2 text-muted hover:text-fg",
        variant === "danger" && "border border-err/40 text-err hover:bg-err/10",
        className,
      )}
    />
  );
}

const field = "rounded-lg border bg-bg/70 px-2.5 py-1.5 text-[13px] outline-none transition-[border-color,box-shadow] focus:border-accent/70 focus:shadow-[0_0_0_3px_rgb(255_92_207/0.15)]";

export function Input({ className, ...p }: React.InputHTMLAttributes<HTMLInputElement>) {
  return <input {...p} className={clsx(field, "placeholder:text-faint w-full", className)} />;
}

export function Textarea({ className, ...p }: React.TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <textarea {...p} className={clsx(field, "font-mono w-full", className)} />;
}

export function Select({ className, children, ...p }: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select {...p} className={clsx(field, className)}>
      {children}
    </select>
  );
}

export function Card({ className, title, actions, children }: { className?: string; title?: React.ReactNode; actions?: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className={clsx("rise rounded-xl border bg-panel/90", className)}>
      {(title || actions) && (
        <div className="flex items-center justify-between gap-3 border-b px-4 py-2.5">
          <div className="text-[13px] font-semibold">{title}</div>
          <div className="flex items-center gap-2">{actions}</div>
        </div>
      )}
      <div className="p-4">{children}</div>
    </div>
  );
}

export function Badge({ children, tone = "muted", className }: { children: React.ReactNode; tone?: "ok" | "warn" | "err" | "info" | "muted" | "accent"; className?: string }) {
  const map = { ok: "bg-ok/15 text-ok", warn: "bg-warn/15 text-warn", err: "bg-err/15 text-err", info: "bg-info/15 text-info", muted: "bg-panel-3 text-muted", accent: "bg-accent/15 text-accent" };
  return <span className={clsx("inline-flex items-center rounded-full px-2 py-0.5 text-[11px] font-medium", map[tone], className)}>{children}</span>;
}

/** A small trend line. Values are plotted left to right, scaled to their own min and max. */
export function Sparkline({ values, color = C.accent, width = 96, height = 30, className }: { values: number[]; color?: string; width?: number; height?: number; className?: string }) {
  const id = useId();
  if (values.length < 2) return null;
  const max = Math.max(...values), min = Math.min(...values), span = max - min || 1;
  const pts = values.map((v, i) => [(i / (values.length - 1)) * width, height - 3 - ((v - min) / span) * (height - 6)] as const);
  const line = pts.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(" ");
  return (
    <svg width={width} height={height} viewBox={`0 0 ${width} ${height}`} className={className} aria-hidden="true">
      <defs><linearGradient id={id} x1="0" y1="0" x2="0" y2="1"><stop offset="0" stopColor={color} stopOpacity="0.35" /><stop offset="1" stopColor={color} stopOpacity="0" /></linearGradient></defs>
      <polygon points={`0,${height} ${line} ${width},${height}`} fill={`url(#${id})`} />
      <polyline points={line} fill="none" stroke={color} strokeWidth="1.8" strokeLinejoin="round" strokeLinecap="round" />
    </svg>
  );
}

export function Stat({ label, value, sub, tone, spark, sparkColor }: { label: string; value: React.ReactNode; sub?: React.ReactNode; tone?: "ok" | "warn" | "err"; spark?: number[]; sparkColor?: string }) {
  return (
    <div className="rise card-hover rounded-xl border bg-panel/90 px-4 py-3.5">
      <div className="text-[12px] text-muted">{label}</div>
      <div className="mt-1 flex items-end gap-3">
        <div className={clsx("text-2xl font-semibold tabular-nums tracking-tight", tone === "err" && "text-err", tone === "warn" && "text-warn", tone === "ok" && "text-ok")}>{value}</div>
        {spark && <Sparkline values={spark} color={sparkColor} className="ml-auto" />}
      </div>
      {sub && <div className="text-[11.5px] text-faint mt-1 truncate">{sub}</div>}
    </div>
  );
}

export function Skeleton({ className }: { className?: string }) {
  return <div className={clsx("shimmer", className ?? "h-4 w-full")} aria-hidden="true" />;
}

export function Table({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <div className={clsx("overflow-auto scroll-thin rounded-xl border bg-panel/60", className)}>
      <table className="w-full text-left text-[12.5px] [&_tbody_tr]:transition-colors">{children}</table>
    </div>
  );
}
export const Th = ({ children, className, ...p }: React.ThHTMLAttributes<HTMLTableCellElement>) => (
  <th {...p} className={clsx("sticky top-0 z-[1] bg-panel-2 px-3 py-2 text-[11px] font-semibold uppercase tracking-[0.06em] text-faint whitespace-nowrap", className)}>{children}</th>
);
export const Td = ({ children, className, ...p }: React.TdHTMLAttributes<HTMLTableCellElement>) => (
  <td {...p} className={clsx("border-t border-border/70 px-3 py-2 align-top", className)}>{children}</td>
);

export function Drawer({ open, onClose, title, children, width = "w-[560px]" }: { open: boolean; onClose: () => void; title?: React.ReactNode; children: React.ReactNode; width?: string }) {
  useEffect(() => {
    if (!open) return;
    const h = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [open, onClose]);
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-40 flex justify-end">
      <div className="absolute inset-0 bg-[#07040f]/60 backdrop-blur-[2px]" onClick={onClose} />
      <div className={clsx("rise relative h-full max-w-full overflow-auto scroll-thin border-l bg-panel shadow-[0_0_60px_rgb(139_92_255/0.18)]", width)}>
        <div className="sticky top-0 z-10 flex items-center justify-between border-b bg-panel/95 backdrop-blur px-4 py-3">
          <div className="font-semibold">{title}</div>
          <button onClick={onClose} aria-label="Close" className="rounded-md p-1 text-muted hover:text-fg hover:bg-panel-2"><X size={16} /></button>
        </div>
        <div className="p-4">{children}</div>
      </div>
    </div>
  );
}

export function Empty({ children }: { children: React.ReactNode }) {
  return (
    <div className="flex flex-col items-center gap-3 rounded-xl border border-dashed p-8 text-center text-muted text-sm">
      <Logo size={40} className="opacity-60" />
      <div className="max-w-md leading-relaxed">{children}</div>
    </div>
  );
}

export function ErrorBox({ error }: { error: unknown }) {
  if (!error) return null;
  const msg = error instanceof Error ? error.message : String(error);
  return <div className="rounded-lg border border-err/40 bg-err/10 px-3 py-2 text-err text-sm">{msg}</div>;
}

export function Label({ children }: { children: React.ReactNode }) {
  return <label className="block text-[11px] font-medium uppercase tracking-[0.06em] text-faint mb-1">{children}</label>;
}

export function Kbd({ children }: { children: React.ReactNode }) {
  return <kbd className="rounded-md border bg-panel-2 px-1.5 py-0.5 font-mono text-[10px] text-muted">{children}</kbd>;
}

/** Title row of a page: the heading, an optional line under it, and actions on the right. */
export function PageHeader({ title, sub, actions }: { title: React.ReactNode; sub?: React.ReactNode; actions?: React.ReactNode }) {
  return (
    <div className="flex flex-wrap items-end justify-between gap-3">
      <div className="min-w-0">
        <h1 className="text-xl font-semibold tracking-tight">{title}</h1>
        {sub && <div className="mt-0.5 text-[13px] text-muted">{sub}</div>}
      </div>
      {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
    </div>
  );
}

/** A row of tabs as a segmented control. */
export function Tabs<T extends string>({ tabs, value, onChange, label }: { tabs: readonly T[]; value: T; onChange: (t: T) => void; label?: (t: T) => React.ReactNode }) {
  return (
    <div className="flex max-w-full gap-1 overflow-x-auto scroll-thin rounded-xl border bg-panel/80 p-1" role="tablist">
      {tabs.map((t) => (
        <button key={t} role="tab" aria-selected={value === t} onClick={() => onChange(t)} className={clsx("shrink-0 rounded-lg px-3 py-1.5 text-[13px] capitalize transition-colors", value === t ? "nav-active font-semibold" : "text-muted hover:text-fg hover:bg-panel-2")}>
          {label ? label(t) : t}
        </button>
      ))}
    </div>
  );
}
