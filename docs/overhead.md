# What Galileo costs the instrumented app

Measured 2026-09-06 on a Mac (Docker Desktop), closed-loop load with `hey` at concurrency 8 for
20 s per configuration, CPU read from the container's cgroup counter so the figure is
CPU-milliseconds actually burned per request, not latency. Harness: `scripts/bench-overhead.sh`.
Each Django figure is the mean of two back-to-back runs; the spread between repetitions of the
same configuration was about ±4 ms, so differences under ~5 ms are noise.

## Django (`galileo-django` 0.2.2), melea `GET /api/consultas/`

The request is heavy on purpose: it runs **218 SQL statements** (an N+1 the detector flags),
so a fully traced request produces ~220 spans. Gunicorn, 3 sync workers, `DEBUG=False`.

| Configuration | CPU / request | vs. off | Throughput | Galileo server CPU / request |
| --- | --- | --- | --- | --- |
| SDK disabled | 101 ms | — | 19.2 rps | — |
| Sample 20 %, no call sites, no SQL params | 107 ms | +6 % | 18.5 rps | 0.75 ms |
| **Sample 20 %, all features** (call sites, SQL params, traced modules, logs) | 111 ms | +10 % | 17.9 rps | 0.9 ms |
| Sample 100 %, all features | 132 ms | +31 % | 15.7 rps | 3.4 ms |
| Sample 100 %, all features, **0.2.1** (before the fixes) | 146 ms | +47 % | 17.5 rps | 4.3 ms |

Per span that works out to roughly **0.14 ms of app CPU** with every feature on, and about
**0.015 ms of Galileo CPU** to ingest it. Memory: +40–50 MB per gunicorn process for the
exporter queues, independent of configuration.

What changed between 0.2.1 and 0.2.2 (both are in `sdk/python`):

- The SQL wrapper used to parse every statement and walk the Python stack even for requests
  that sampling had already dropped, so `GALILEO_SAMPLE_RATIO` barely moved CPU. It now returns
  immediately when the current span is not recording; sampling is a real CPU knob.
- `GALILEO_CALL_SITES=0` now also skips the stack walk inside the SQL wrapper (it only removed
  the span processor before).
- The statement text is sent once (`db.query.text`) instead of twice and is capped at 2 KB;
  that alone took the fully traced request from 146 ms to 132 ms.

## Go (`internal/platform/otel` in melea-api-go), `GET /api/health/` (1 SQL statement)

| Configuration | CPU / request | Throughput |
| --- | --- | --- |
| Telemetry off | 0.178 ms | 13 990 rps |
| Telemetry on (2 spans) | 0.220 ms | 11 730 rps |

About **0.04 ms per request**, or 0.02 ms per span. On an endpoint that does real work this
disappears into the noise; the Go SDK is roughly 7× cheaper per span than the Python one
because there is no stack walk and no per-statement parsing.

## Reading the numbers

- Overhead scales with **spans per request**, not with requests. A request that runs 218
  queries pays 218 times. Fixing the N+1 (Traces → N+1 candidates, or the assistant's
  Investigate) is worth more than any SDK setting, for the app and for Galileo.
- On a 1 vCPU host running the Python app, prefer **all features with `GALILEO_SAMPLE_RATIO`
  between 0.2 and 0.5** over switching features off: the sampled configurations cost about the
  same and keep call sites and SQL parameters on the traces you do get. Errors are still
  counted by the request metrics when a trace is sampled out.
- The Galileo server spends ~3.4 ms CPU per fully traced 220-span request; a service doing
  50 such requests per second would consume about one sixth of a core on the Galileo host.

## Reproducing

```bash
# from the directory of the app under test; the app container must expose /sys/fs/cgroup/cpu.stat
scripts/bench-overhead.sh "<label>" <container> <url> <host header> "<Header: v|Header2: v>" 20 8
```

Results append to `bench-results.jsonl` in the current directory. Recreate the container between
configurations (env is read once at start), warm it, and repeat each configuration at least twice.
