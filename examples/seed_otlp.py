#!/usr/bin/env python3
"""Seed Galileo with realistic-looking traces, logs and metrics over OTLP/HTTP JSON.

Usage: GALILEO_KEY=glk_... python3 examples/seed_otlp.py [--minutes 60] [--traces 400]
"""
import argparse, json, os, random, time, urllib.request, uuid

KEY = os.environ.get("GALILEO_KEY", "dev")
BASE = os.environ.get("GALILEO_OTLP", "http://127.0.0.1:4318")

SERVICES = {
    "api": ["GET /pets/{id}", "POST /pets", "GET /owners/{id}", "POST /login", "GET /search"],
    "worker": ["process invoice", "send email", "sync inventory"],
}
TENANTS = ["acme", "globex", "initech", "umbrella"]
USERS = [f"user-{i}" for i in range(1, 40)]
MODELS = ["claude-sonnet-5", "gpt-4o-mini", "llama3.2"]

def hexid(n): return uuid.uuid4().hex[: n * 2]
def kv(k, v):
    if isinstance(v, bool): return {"key": k, "value": {"boolValue": v}}
    if isinstance(v, int): return {"key": k, "value": {"intValue": str(v)}}
    if isinstance(v, float): return {"key": k, "value": {"doubleValue": v}}
    return {"key": k, "value": {"stringValue": str(v)}}

def post(path, body):
    req = urllib.request.Request(f"{BASE}{path}", data=json.dumps(body).encode(), method="POST",
                                 headers={"content-type": "application/json", "authorization": f"Bearer {KEY}"})
    with urllib.request.urlopen(req) as r:
        return r.status

def make_trace(t_ns):
    svc = random.choices(list(SERVICES), weights=[4, 1])[0]
    name = random.choice(SERVICES[svc])
    tenant = random.choice(TENANTS)
    user = random.choice(USERS)
    # tenant "umbrella" on search is slow, that's the story BubbleUp should find
    slow = tenant == "umbrella" and "search" in name
    root_dur = int(random.lognormvariate(3.5 if not slow else 6.0, 0.5) * 1e6)
    error = random.random() < (0.25 if slow else 0.02)
    trace_id = hexid(16); root_id = hexid(8)
    spans, logs = [], []
    resource = [kv("service.name", svc), kv("service.version", "1.4.2"), kv("deployment.environment.name", "prod"), kv("host.name", f"{svc}-{random.randint(1,3)}")]
    attrs = [kv("tenant.id", tenant), kv("user.id", user)]
    if svc == "api":
        route = name.split(" ", 1)[1]; method = name.split(" ", 1)[0]
        status = 500 if error else (404 if random.random() < 0.05 else 200)
        attrs += [kv("http.request.method", method), kv("http.route", route), kv("url.path", route.replace("{id}", str(random.randint(1, 999)))),
                  kv("http.response.status_code", status), kv("app.cart_size", random.randint(0, 12)), kv("app.plan", random.choice(["free", "pro", "enterprise"]))]
    spans.append({"traceId": trace_id, "spanId": root_id, "name": name, "kind": 2 if svc == "api" else 5,
                  "startTimeUnixNano": str(t_ns), "endTimeUnixNano": str(t_ns + root_dur), "attributes": attrs,
                  "status": {"code": 2, "message": "upstream timeout"} if error else {"code": 1},
                  "events": [{"timeUnixNano": str(t_ns + root_dur - 1000), "name": "exception",
                              "attributes": [kv("exception.type", "TimeoutError"), kv("exception.message", "upstream timeout after 5s")]}] if error else []})
    # db child
    off = int(root_dur * 0.1); dur = int(root_dur * random.uniform(0.2, 0.6))
    spans.append({"traceId": trace_id, "spanId": hexid(8), "parentSpanId": root_id, "name": "SELECT pets", "kind": 3,
                  "startTimeUnixNano": str(t_ns + off), "endTimeUnixNano": str(t_ns + off + dur),
                  "attributes": [kv("db.system", "postgresql"), kv("db.query.text", "SELECT * FROM pets WHERE id = $1"), kv("tenant.id", tenant)]})
    # llm child on some requests
    if random.random() < 0.3:
        model = random.choice(MODELS); off2 = off + dur; ldur = int(random.lognormvariate(6.5, 0.4) * 1e6)
        tin, tout = random.randint(200, 2000), random.randint(50, 600)
        cost = (tin * 3 + tout * 15) / 1e6 if "claude" in model else (tin * 0.15 + tout * 0.6) / 1e6 if "gpt" in model else 0.0
        spans.append({"traceId": trace_id, "spanId": hexid(8), "parentSpanId": root_id, "name": f"chat {model}", "kind": 3,
                      "startTimeUnixNano": str(t_ns + off2), "endTimeUnixNano": str(t_ns + off2 + ldur),
                      "attributes": [kv("gen_ai.system", "anthropic" if "claude" in model else "openai" if "gpt" in model else "ollama"),
                                     kv("gen_ai.operation.name", "chat"), kv("gen_ai.request.model", model), kv("gen_ai.response.model", model),
                                     kv("gen_ai.usage.input_tokens", tin), kv("gen_ai.usage.output_tokens", tout), kv("gen_ai.usage.cost_usd", round(cost, 6)),
                                     kv("gen_ai.galileo.route", "assistant"), kv("tenant.id", tenant)]})
    logs.append({"timeUnixNano": str(t_ns + 1000), "severityNumber": 17 if error else 9, "severityText": "ERROR" if error else "INFO",
                 "body": {"stringValue": f"{'failed' if error else 'handled'} {name} for tenant {tenant} in {root_dur/1e6:.1f}ms"},
                 "traceId": trace_id, "spanId": root_id, "attributes": [kv("tenant.id", tenant), kv("user.id", user)]})
    return resource, spans, logs

def main():
    ap = argparse.ArgumentParser(); ap.add_argument("--minutes", type=int, default=60); ap.add_argument("--traces", type=int, default=400)
    a = ap.parse_args()
    now = time.time_ns(); span_batches = {}; log_batches = {}
    for i in range(a.traces):
        t_ns = now - random.randint(0, a.minutes * 60) * 1_000_000_000
        res, spans, logs = make_trace(t_ns)
        key = res[0]["value"]["stringValue"] + res[3]["value"]["stringValue"]
        span_batches.setdefault(key, (res, []))[1].extend(spans); log_batches.setdefault(key, (res, []))[1].extend(logs)
    post("/v1/traces", {"resourceSpans": [{"resource": {"attributes": r}, "scopeSpans": [{"scope": {"name": "seed"}, "spans": s}]} for r, s in span_batches.values()]})
    post("/v1/logs", {"resourceLogs": [{"resource": {"attributes": r}, "scopeLogs": [{"scope": {"name": "seed"}, "logRecords": l}]} for r, l in log_batches.values()]})
    # metrics: a gauge and a counter per host, every 30s
    points_g, points_c = [], []
    for host in ["api-1", "api-2", "api-3"]:
        for t in range(0, a.minutes * 60, 30):
            ts = str(now - t * 1_000_000_000)
            points_g.append({"attributes": [kv("host.name", host)], "timeUnixNano": ts, "asDouble": round(random.uniform(0.2, 0.9), 3)})
            points_c.append({"attributes": [kv("host.name", host), kv("http.route", "/search")], "timeUnixNano": ts, "asInt": str(random.randint(5, 40))})
    post("/v1/metrics", {"resourceMetrics": [{"resource": {"attributes": [kv("service.name", "api")]}, "scopeMetrics": [{"scope": {"name": "seed"}, "metrics": [
        {"name": "process.cpu.utilization", "unit": "1", "gauge": {"dataPoints": points_g}},
        {"name": "http.server.requests", "unit": "{request}", "sum": {"dataPoints": points_c, "aggregationTemporality": 1, "isMonotonic": True}}]}]}]})
    print(f"seeded {a.traces} traces, {len(points_g)+len(points_c)} metric points over {a.minutes} minutes")

if __name__ == "__main__":
    main()
