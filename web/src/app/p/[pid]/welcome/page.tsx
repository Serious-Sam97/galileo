"use client";

import { useEffect, useState } from "react";
import Link from "next/link";
import { useRouter } from "next/navigation";
import clsx from "clsx";
import { Check, Loader2 } from "lucide-react";
import { get, post, API_BASE, OTLP_BASE } from "@/lib/api";
import { useProjectId, useProjectQuery, useProjectMutation } from "@/lib/hooks";
import { Button, Card, ErrorBox } from "@/components/ui";
import { useT } from "@/lib/i18n";

type Stack = "django" | "python" | "node" | "php" | "rust" | "android" | "browser";
const STACKS: { id: Stack; label: string; hint: string }[] = [
  { id: "django", label: "Django", hint: "galileo-django" },
  { id: "python", label: "Python (FastAPI, Flask…)", hint: "galileo-python" },
  { id: "node", label: "Node.js", hint: "@galileo/node" },
  { id: "php", label: "PHP / Laravel", hint: "galileo/php" },
  { id: "rust", label: "Rust", hint: "galileo crate" },
  { id: "android", label: "Android", hint: "galileo-android" },
  { id: "browser", label: "Browser", hint: "galileo-rum.js" },
];

function snippet(stack: Stack, key: string, host: string, otlp: string): string {
  switch (stack) {
    case "django": return `pip install ./sdk/python   # galileo-django\n\n# settings.py\nINSTALLED_APPS += ["galileo_django"]\nMIDDLEWARE = ["galileo_django.middleware.GalileoContextMiddleware", *MIDDLEWARE]\n\n# environment\nGALILEO_OTLP_ENDPOINT=${otlp}\nGALILEO_API_KEY=${key}\nOTEL_SERVICE_NAME=my-api`;
    case "python": return `pip install ./sdk/python-generic   # galileo-python\n\nimport galileo\ngalileo.init(endpoint="${otlp}", api_key="${key}", service="my-api")\n# FastAPI: from galileo.fastapi import GalileoMiddleware; app.add_middleware(GalileoMiddleware)\n# Flask:   import galileo.flask; galileo.flask.instrument(app)`;
    case "node": return `npm i ./sdk/node   # @galileo/node\n\n// first line of your entry file (or node --import @galileo/node/register)\nimport { init } from "@galileo/node";\ninit({ endpoint: "${otlp}", apiKey: "${key}", service: "my-api" });`;
    case "php": return `composer require galileo/php\n\n// bootstrap\n\\Galileo\\Galileo::init(['endpoint' => '${otlp}', 'api_key' => '${key}', 'service' => 'my-api']);\n// Laravel: add Galileo\\Laravel\\GalileoServiceProvider and set GALILEO_ENDPOINT / GALILEO_API_KEY in .env`;
    case "rust": return `# Cargo.toml: galileo = { path = "…/galileo/sdk/rust/galileo" }\n\nlet _guard = galileo::init(galileo::Config::from_env().endpoint("${otlp}").api_key("${key}").service("my-api"));\n// axum: .layer(axum::middleware::from_fn(galileo::axum::middleware))`;
    case "android": return `// sdk/android/galileo (AAR)\nGalileo.init(this, endpoint = "${otlp}", apiKey = "${key}", service = "my-app")\n// OkHttp: .addInterceptor(Galileo.okHttpInterceptor(propagateTo = listOf("https://api.example.com")))`;
    case "browser": return `<script src="${host}/rum.js" data-key="${key}" data-service="my-site"\n        data-endpoint="${otlp}"></script>`;
  }
}

export default function WelcomePage() {
  const t = useT();
  const pid = useProjectId();
  const router = useRouter();
  const [stack, setStack] = useState<Stack>("python");
  const [step, setStep] = useState(0);
  const [key, setKey] = useState("");
  const keys = useProjectQuery<{ keys?: { id: string }[]; api_keys?: { id: string }[] }>(["api-keys"], "/api-keys");
  const createKey = useProjectMutation<{ name: string; scopes: string[] }>((p, b) => post<{ key: string }>(`/api/projects/${p}/api-keys`, b), [["api-keys"]]);
  const [spans, setSpans] = useState(0);
  useEffect(() => {
    if (step < 2) return;
    let alive = true;
    const tick = () => get<{ today?: { spans?: number } }>(`/api/projects/${pid}/usage`).then((u) => { if (alive) setSpans(u.today?.spans ?? 0); }).catch(() => {});
    tick();
    const h = step === 2 ? setInterval(tick, 3000) : undefined;
    return () => { alive = false; if (h) clearInterval(h); };
  }, [step, pid]);
  const gotTrace = spans > 0;
  useEffect(() => { if (step === 2 && gotTrace) setStep(3); }, [step, gotTrace]);
  const createBoard = useProjectMutation<string>((p, tpl) => post<{ board: { id: string } }>(`/api/projects/${p}/boards/templates`, { kind: tpl }), [["boards"]]);

  async function makeKey() {
    const r = await createKey.mutateAsync({ name: "onboarding", scopes: stack === "browser" ? ["rum"] : ["ingest", "gateway"] });
    setKey((r as { key: string }).key);
    setStep(1);
  }
  const steps = [t("Get started"), "Install", "First trace", "First board"];
  return (
    <div className="mx-auto max-w-3xl space-y-4">
      <div>
        <h1 className="text-lg font-semibold">{t("Get started")}</h1>
        <p className="text-sm text-muted">Four steps from an empty project to a board with real data.</p>
      </div>
      <ol className="flex gap-2 text-xs">
        {steps.map((s, i) => <li key={s} className={clsx("flex items-center gap-1 rounded-full border px-2 py-1", i === step ? "border-accent text-accent" : i < step ? "text-ok" : "text-muted")}>{i < step ? <Check size={12} /> : <span className="font-mono">{i + 1}</span>} {s}</li>)}
      </ol>
      {step === 0 && (
        <Card title="1 · Pick your stack">
          <div className="grid gap-2 sm:grid-cols-2 md:grid-cols-3">
            {STACKS.map((s) => <button key={s.id} onClick={() => setStack(s.id)} className={clsx("rounded-lg border p-3 text-left", stack === s.id ? "border-accent bg-accent/10" : "hover:bg-panel-2")}><div className="text-sm font-medium">{s.label}</div><div className="text-[11px] text-muted font-mono">{s.hint}</div></button>)}
          </div>
          <ErrorBox error={createKey.error} />
          <div className="mt-3 flex gap-2 items-center"><Button variant="primary" onClick={makeKey} disabled={createKey.isPending}>Create a key and continue</Button>{((keys.data?.api_keys ?? keys.data?.keys)?.length ?? 0) > 0 && <span className="text-xs text-muted">this project already has {(keys.data?.api_keys ?? keys.data?.keys)?.length} key(s); a new one named "onboarding" is created</span>}</div>
        </Card>
      )}
      {step === 1 && (
        <Card title="2 · Install and point the SDK at Galileo">
          <p className="text-sm text-muted mb-2">The key below is shown once. It goes into your app's environment, never into the browser (except the RUM key, which is scoped to the browser only).</p>
          <pre className="whitespace-pre-wrap rounded border bg-bg p-3 font-mono text-[11px]">{snippet(stack, key, API_BASE, OTLP_BASE)}</pre>
          <p className="text-xs text-muted mt-2">Full details per stack: <Link className="underline" href={`/p/${pid}/settings`}>Settings → Connect</Link>.</p>
          <div className="mt-3 flex gap-2"><Button onClick={() => setStep(0)}>Back</Button><Button variant="primary" onClick={() => setStep(2)}>Done, waiting for traffic</Button></div>
        </Card>
      )}
      {step === 2 && (
        <Card title="3 · Waiting for the first trace">
          <div className="flex items-center gap-2 text-sm"><Loader2 className="animate-spin text-accent" size={16} /> Listening on {OTLP_BASE} — make a request to your app. This page checks every 3 seconds.</div>
          <p className="text-xs text-muted mt-2">Nothing arriving? Check the endpoint ({OTLP_BASE}), the key, and that the SDK flushes on exit for short scripts. Spans seen in the last hour: {spans}.</p>
          <div className="mt-3 flex gap-2"><Button onClick={() => setStep(1)}>Back</Button><Button onClick={() => setStep(3)}>Skip</Button></div>
        </Card>
      )}
      {step === 3 && (
        <Card title="4 · First board">
          <p className="text-sm text-muted mb-2">{gotTrace ? `Traces are flowing (${spans} spans in the last hour).` : "No traces yet, but you can prepare the board."} Create a RED board (requests, errors, duration per route) from a template.</p>
          <ErrorBox error={createBoard.error} />
          <div className="flex gap-2">
            <Button variant="primary" onClick={async () => { const r = await createBoard.mutateAsync(stack === "browser" ? "browser" : "red"); router.push(`/p/${pid}/boards/${(r as { board: { id: string } }).board.id}`); }} disabled={createBoard.isPending}>Create board and open it</Button>
            <Button onClick={() => router.push(`/p/${pid}/overview`)}>Go to Overview</Button>
          </div>
        </Card>
      )}
    </div>
  );
}
