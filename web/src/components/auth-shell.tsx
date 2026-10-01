import { Logo } from "@/components/logo";

/** Full-screen frame for the signed-out pages (login, invite): sky, horizon grid, the form in a card. */
export function AuthShell({ children }: { children: React.ReactNode }) {
  return (
    <div className="relative flex flex-1 items-center justify-center overflow-hidden p-6" style={{ background: "radial-gradient(90% 70% at 50% 0%, #3a1763 0%, #160b2b 45%, #0d0818 75%)" }}>
      <div className="pointer-events-none absolute left-1/2 top-[18%] h-[360px] w-[360px] -translate-x-1/2 rounded-full opacity-40 blur-3xl" style={{ background: "radial-gradient(circle, #ff5ccf, transparent 65%)" }} aria-hidden="true" />
      <div className="vapor-grid pointer-events-none absolute inset-x-0 bottom-0 h-[38vh] opacity-40 [transform:perspective(420px)_rotateX(55deg)] origin-bottom" aria-hidden="true" />
      <div className="relative w-full max-w-sm space-y-6">
        <div className="flex flex-col items-center gap-3 text-center">
          <Logo size={64} tile className="drop-shadow-[0_0_24px_rgb(255_92_207/0.45)]" />
          <div>
            <div className="text-2xl font-bold tracking-tight">Galileo</div>
            <div className="text-[13px] text-muted">See what your services are doing, and why.</div>
          </div>
        </div>
        <div className="rise rounded-2xl border bg-panel/80 p-6 shadow-[0_20px_60px_rgb(0_0_0/0.45)] backdrop-blur-md">{children}</div>
      </div>
    </div>
  );
}
