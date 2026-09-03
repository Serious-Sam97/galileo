"use client";

import clsx from "clsx";
import { X } from "lucide-react";
import { useEffect } from "react";

type BtnProps = React.ButtonHTMLAttributes<HTMLButtonElement> & { variant?: "primary" | "ghost" | "danger" | "outline"; size?: "sm" | "md" };
export function Button({ variant = "outline", size = "md", className, ...p }: BtnProps) {
  return (
    <button
      {...p}
      className={clsx(
        "inline-flex items-center gap-1.5 rounded-md font-medium transition-colors disabled:opacity-50 disabled:cursor-not-allowed whitespace-nowrap",
        size === "sm" ? "px-2 py-1 text-xs" : "px-3 py-1.5 text-[13px]",
        variant === "primary" && "bg-accent text-accent-fg hover:brightness-110",
        variant === "outline" && "border bg-panel hover:bg-panel-2",
        variant === "ghost" && "hover:bg-panel-2 text-muted hover:text-fg",
        variant === "danger" && "border border-err/40 text-err hover:bg-err/10",
        className,
      )}
    />
  );
}

export function Input({ className, ...p }: React.InputHTMLAttributes<HTMLInputElement>) {
  return <input {...p} className={clsx("rounded-md border bg-bg px-2 py-1.5 text-[13px] outline-none focus:border-accent placeholder:text-muted/60 w-full", className)} />;
}

export function Textarea({ className, ...p }: React.TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <textarea {...p} className={clsx("rounded-md border bg-bg px-2 py-1.5 text-[13px] outline-none focus:border-accent font-mono w-full", className)} />;
}

export function Select({ className, children, ...p }: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return (
    <select {...p} className={clsx("rounded-md border bg-bg px-2 py-1.5 text-[13px] outline-none focus:border-accent", className)}>
      {children}
    </select>
  );
}

export function Card({ className, title, actions, children }: { className?: string; title?: React.ReactNode; actions?: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className={clsx("rounded-lg border bg-panel", className)}>
      {(title || actions) && (
        <div className="flex items-center justify-between border-b px-3 py-2">
          <div className="text-xs font-semibold uppercase tracking-wide text-muted">{title}</div>
          <div className="flex items-center gap-2">{actions}</div>
        </div>
      )}
      <div className="p-3">{children}</div>
    </div>
  );
}

export function Badge({ children, tone = "muted", className }: { children: React.ReactNode; tone?: "ok" | "warn" | "err" | "info" | "muted" | "accent"; className?: string }) {
  const map = { ok: "bg-ok/15 text-ok", warn: "bg-warn/15 text-warn", err: "bg-err/15 text-err", info: "bg-info/15 text-info", muted: "bg-panel-2 text-muted", accent: "bg-accent/15 text-accent" };
  return <span className={clsx("inline-flex items-center rounded px-1.5 py-0.5 text-[11px] font-medium", map[tone], className)}>{children}</span>;
}

export function Stat({ label, value, sub, tone }: { label: string; value: React.ReactNode; sub?: React.ReactNode; tone?: "ok" | "warn" | "err" }) {
  return (
    <div className="rounded-lg border bg-panel px-3 py-2.5">
      <div className="text-[11px] uppercase tracking-wide text-muted">{label}</div>
      <div className={clsx("mt-1 text-xl font-semibold tabular-nums", tone === "err" && "text-err", tone === "warn" && "text-warn", tone === "ok" && "text-ok")}>{value}</div>
      {sub && <div className="text-[11px] text-muted mt-0.5">{sub}</div>}
    </div>
  );
}

export function Table({ children, className }: { children: React.ReactNode; className?: string }) {
  return (
    <div className={clsx("overflow-auto scroll-thin rounded-md border", className)}>
      <table className="w-full text-left text-[12.5px]">{children}</table>
    </div>
  );
}
export const Th = ({ children, className, ...p }: React.ThHTMLAttributes<HTMLTableCellElement>) => (
  <th {...p} className={clsx("sticky top-0 bg-panel-2 px-2 py-1.5 text-[11px] font-semibold uppercase tracking-wide text-muted whitespace-nowrap", className)}>{children}</th>
);
export const Td = ({ children, className, ...p }: React.TdHTMLAttributes<HTMLTableCellElement>) => (
  <td {...p} className={clsx("border-t px-2 py-1.5 align-top", className)}>{children}</td>
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
      <div className="absolute inset-0 bg-black/50" onClick={onClose} />
      <div className={clsx("relative h-full max-w-full overflow-auto scroll-thin border-l bg-panel shadow-2xl", width)}>
        <div className="sticky top-0 z-10 flex items-center justify-between border-b bg-panel px-4 py-3">
          <div className="font-semibold">{title}</div>
          <button onClick={onClose} className="text-muted hover:text-fg"><X size={16} /></button>
        </div>
        <div className="p-4">{children}</div>
      </div>
    </div>
  );
}

export function Empty({ children }: { children: React.ReactNode }) {
  return <div className="rounded-md border border-dashed p-6 text-center text-muted text-sm">{children}</div>;
}

export function ErrorBox({ error }: { error: unknown }) {
  if (!error) return null;
  const msg = error instanceof Error ? error.message : String(error);
  return <div className="rounded-md border border-err/40 bg-err/10 px-3 py-2 text-err text-sm">{msg}</div>;
}

export function Label({ children }: { children: React.ReactNode }) {
  return <label className="block text-[11px] uppercase tracking-wide text-muted mb-1">{children}</label>;
}

export function Kbd({ children }: { children: React.ReactNode }) {
  return <kbd className="rounded border bg-panel-2 px-1 py-0.5 font-mono text-[10px] text-muted">{children}</kbd>;
}
