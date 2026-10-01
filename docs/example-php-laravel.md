# Example: integrating a PHP (Laravel) app with Galileo

A complete, worked integration of a Laravel API with a hosted Galileo — the same steps apply to
any other service; only the SDK section changes (see [integrating.md](integrating.md) for other
stacks). The example app is **shop-api**, a multi-tenant Laravel 11 API on PHP-FPM that:

- serves a JSON API to a Blade/JS front-end,
- stores data in PostgreSQL,
- calls another internal service (`billing`) over HTTP,
- runs queue workers,
- summarises support tickets with an LLM.

Galileo is hosted with the UI at `https://galileo.serious-sam.dev` and everything apps talk to at
`https://galileo-api.serious-sam.dev`. Apps only ever need the **API** hostname. For a local Galileo
replace `https://galileo-api.serious-sam.dev/otlp` with `http://localhost:4318` and
`https://galileo-api.serious-sam.dev` with `http://localhost:8080`.

What you get at the end:

| In Galileo | Comes from |
| --- | --- |
| One trace per request, named by route (`GET /api/orders/{order}`) | SDK middleware |
| Every SQL query as a span, with the PHP function and file:line that ran it | `DB::listen` hook |
| `user.id`, `user.email`, `tenant.id` on every span and log line | `Auth::user()` |
| Exceptions grouped as Issues, with users/tenants affected and first-seen release | exception handler hook |
| Logs correlated to traces | Monolog handler |
| Calls to `billing` inside the same trace | `traceparent` on outgoing HTTP |
| LLM calls with tokens, cost and latency in the same trace; model switchable without deploys | LLM gateway |
| Browser sessions linked to the API requests they made | `rum.js` |
| Deploy markers on every chart | one `curl` in CI |

---

## 1. Galileo side (once)

1. **Project**: org page → New project → `shop`.
2. **API keys** (Settings → API keys → New key). One key per purpose, each with the minimum scope:

   | Key name | Scopes | Lives in |
   | --- | --- | --- |
   | `shop-api production` | `ingest`, `gateway` | the API servers' environment |
   | `shop-web` | `rum` | the HTML page (public by design) |
   | `shop-api ci` | `deploy` | the CI secret store |

   Each key (`glk_…`) is shown once. Keep it out of the repository.
3. **LLM route** (only if you use the gateway): Settings → AI → Providers → add e.g. Anthropic or
   OpenAI with the provider's own key. Then Settings → AI → Routes → New route, alias
   `shop-ticket-summary`, a target model, a fallback, a daily budget, and PII redaction if tickets
   may contain personal data. The app will send `model: "shop-ticket-summary"`; which real model
   answers is decided here.
4. **Redaction** (Settings → Redaction): add a rule for any attribute that can carry PII beyond
   the built-in defaults (passwords, tokens, card numbers are covered already).

Check the key and the network path from the app host before touching the app:

```bash
curl -s -o /dev/null -w '%{http_code}\n' -X POST https://galileo-api.serious-sam.dev/otlp/v1/traces \
  -H 'content-type: application/json' -H "Authorization: Bearer $GALILEO_API_KEY" \
  -d '{"resourceSpans":[]}'
# 200 = reachable and key valid · 401 = wrong key · 403 = key lacks the ingest scope · 404 = missing /otlp
```

## 2. Install the SDK

`galileo/php` needs PHP ≥ 8.1 and Guzzle (pulled in automatically). It lives in this repository
(`sdk/php`), so point Composer at it — either a path next to your app or the Git repository:

```jsonc
// composer.json of shop-api
"repositories": [
  { "type": "path", "url": "../galileo/sdk/php" }          // or { "type": "vcs", "url": "git@github.com:<you>/galileo.git" }
],
```

```bash
composer require galileo/php:@dev
```

The Laravel service provider is auto-discovered; nothing to register.

## 3. Configure

### `.env`

```dotenv
GALILEO_ENDPOINT=https://galileo-api.serious-sam.dev/otlp
GALILEO_API_KEY=glk_...                  # "shop-api production" (ingest + gateway)
OTEL_SERVICE_NAME=shop-api
GALILEO_ENV=prod                          # deployment.environment on every span
GALILEO_RELEASE=2026.09.23-1              # service.version; same string as the deploy marker
GALILEO_SQL_PARAMS=1                      # optional: record bound parameters (redacted at ingest)

# gateway (step 7)
GALILEO_GATEWAY_URL=https://galileo-api.serious-sam.dev/gw
GALILEO_LLM_ROUTE=shop-ticket-summary
```

The SDK is **inert without `GALILEO_API_KEY`**, so local development and CI stay untouched until you
set it.

### `config/galileo.php` — required if you run `php artisan config:cache`

With a cached config Laravel no longer loads `.env`, so the SDK only sees what is in `config()` or
in the real process environment. Add this file so the key and endpoint survive caching:

```php
<?php
return [
    'endpoint' => env('GALILEO_ENDPOINT'),
    'api_key'  => env('GALILEO_API_KEY'),
    'service'  => env('OTEL_SERVICE_NAME', 'shop-api'),
    'env'      => env('GALILEO_ENV'),
    'release'  => env('GALILEO_RELEASE'),
];
```

`GALILEO_SQL_PARAMS` is read from the process environment. Under PHP-FPM, which starts workers with
an empty environment (`clear_env = yes`), pass it in the pool config (or set it in the container's
environment):

```ini
; /etc/php/8.3/fpm/pool.d/www.conf
env[GALILEO_SQL_PARAMS] = 1
```

Restart FPM and clear/rebuild the config cache after changing any of this.

## 4. What is automatic

With only steps 2–3 done, deploy and send a request. The provider adds:

- **A server span per request** in the `web` and `api` middleware groups, renamed to the route
  template once routing is done (`GET /api/orders/{order}`), with `http.response.status_code`,
  `client.address`, `user_agent.original`, and an error status on 5xx. The response carries
  `x-galileo-trace-id`, handy for support tickets and logs.
- **A client span per SQL query** from `DB::listen`: `db.system`, `db.statement`, `db.sql.table`,
  and the call site that issued it (`code.function.name`, `code.namespace`, `code.file.path`,
  `code.line.number`, skipping `vendor/`). The trace page flags repeated queries (N+1) with a badge.
- **Identity**: after the request is handled, `Auth::user()` becomes `user.id`, `user.email`,
  `user.name` and — if the model has a `tenant_id` attribute — `tenant.id`.
- **Exceptions**: everything reported by Laravel's handler becomes an exception span, which Galileo
  groups into Issues.
- **Incoming `traceparent`** is honoured, so a request made by an instrumented browser or another
  service continues that trace instead of starting a new one.

### Identity when the tenant is not `tenant_id`

If the tenant comes from a header, a subdomain or a different column, set identity yourself in a
middleware that runs after authentication. Identity set by the app during the request takes
precedence over the `Auth::user()` default:

```php
use Galileo\Identity;

final class GalileoIdentity
{
    public function handle($request, \Closure $next)
    {
        if ($u = $request->user()) {
            Identity::set($u->id, $u->email, $u->name, $request->attributes->get('clinic_id'));
        }
        return $next($request);
    }
}
```

Keep e-mails out if you do not want them in telemetry: pass `null` as the second argument.

## 5. Logs

Add a Monolog channel that ships to Galileo, and put it in your stack so existing `Log::` calls go
there too:

```php
// config/logging.php
'channels' => [
    'stack' => ['driver' => 'stack', 'channels' => ['daily', 'galileo']],
    'galileo' => [
        'driver'  => 'monolog',
        'handler' => Galileo\MonologHandler::class,
        'level'   => 'info',
    ],
],
```

Every record carries the active trace id and the current identity, and a `Throwable` in the log
context becomes `exception.*` attributes:

```php
Log::warning('payment retry', ['order_id' => $order->id, 'attempt' => 2]);
Log::error('refund failed', ['exception' => $e]);
```

In Galileo: Logs → filter `service.name = shop-api` → click a line → Open trace.

## 6. Your own spans, and calls to other services

Wrap business steps that matter so they show up in the waterfall. The closure's return value is
passed through; an exception marks the span as failed and is rethrown.

```php
use Galileo\Galileo;
use OpenTelemetry\API\Trace\SpanKind;

$total = Galileo::span('price cart', fn () => $this->pricing->price($cart), attributes: [
    'cart.items' => count($cart->items),
]);
```

**Calls to another service** (here `billing`) join the same trace when the request carries the
current `traceparent`. With Laravel's HTTP client:

```php
use Galileo\Galileo;
use Illuminate\Support\Facades\Http;
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Trace\SpanKind;

$invoice = Galileo::span('POST billing /invoices', function () use ($order) {
    $headers = [];
    Globals::propagator()->inject($headers);          // adds traceparent (and tracestate)
    return Http::withHeaders($headers)
        ->post('https://billing.internal/invoices', ['order_id' => $order->id])
        ->throw()
        ->json();
}, SpanKind::KIND_CLIENT, ['server.address' => 'billing.internal']);
```

If `billing` is instrumented with any Galileo SDK or plain OpenTelemetry, its spans appear under
this one; the Services map page draws the `shop-api → billing` edge.

## 7. LLM calls through the gateway

Point the HTTP call at Galileo instead of the provider and use the route alias as the model. The
Galileo key replaces the provider key (which stays in Galileo). Send `traceparent` so the call
lands inside the request trace, and the identity headers so cost and budgets are per user/tenant:

```php
use Galileo\Identity;
use Illuminate\Support\Facades\Http;
use OpenTelemetry\API\Globals;

function summariseTicket(string $text): string
{
    $headers = [];
    Globals::propagator()->inject($headers);
    $who = Identity::attributes();

    $res = Http::withToken(config('galileo.api_key'))
        ->withHeaders($headers + array_filter([
            'x-galileo-user-id'   => $who['user.id'] ?? null,
            'x-galileo-tenant-id' => $who['tenant.id'] ?? null,
        ]))
        ->timeout(60)
        ->post(env('GALILEO_GATEWAY_URL') . '/v1/chat/completions', [
            'model'    => env('GALILEO_LLM_ROUTE', 'shop-ticket-summary'),
            'messages' => [
                ['role' => 'system', 'content' => 'Summarise the support ticket in two sentences.'],
                ['role' => 'user', 'content' => $text],
            ],
        ])
        ->throw();

    return $res->json('choices.0.message.content');
}
```

(Put `GALILEO_GATEWAY_URL` and the route in `config/galileo.php` too if you cache config.)

- The body is the OpenAI chat format; the gateway translates it for Anthropic or any other
  provider behind the route. Anthropic's format works at `/v1/messages`.
- `x-galileo-conversation-id` groups multi-turn calls into one conversation in AI → Agents.
- Response headers tell you what happened: `x-galileo-provider`, `x-galileo-model`,
  `x-galileo-cache` (`hit`/`miss`), `x-galileo-trace-id`.
- Errors come back in the client's format: 429 when the route's budget or rate limit is reached,
  403 for a disabled route, 502 when every target failed.
- Using `openai-php/client` instead: `OpenAI::factory()->withApiKey($galileoKey)->withBaseUri('https://galileo-api.serious-sam.dev/gw/v1')->withHttpHeader('traceparent', $headers['traceparent'] ?? '')->make()`.

## 8. Queue workers and artisan commands

Workers have no HTTP request, so the middleware never runs; queries would otherwise arrive as
loose single-span traces. Wrap each job in a span and set identity from the job's data:

```php
use Galileo\Galileo;
use Galileo\Identity;
use OpenTelemetry\API\Trace\SpanKind;

final class SendInvoice implements ShouldQueue
{
    public function handle(): void
    {
        Identity::set($this->userId, null, null, $this->tenantId);
        try {
            Galileo::span('job SendInvoice', fn () => $this->send(), SpanKind::KIND_CONSUMER, [
                'messaging.system' => 'laravel-queue',
                'order.id' => $this->orderId,
            ]);
        } finally {
            Identity::clear();          // workers are long-lived: do not leak identity to the next job
        }
    }
}
```

Long-running workers export in batches in the background; nothing else to do. For a one-off
command, the SDK flushes on shutdown.

## 9. Browser sessions (optional)

In the main Blade layout, with the **rum** key (safe to publish; it can only ingest browser data):

```html
<script src="https://galileo-api.serious-sam.dev/rum.js"
        data-key="glk_<rum key>" data-service="shop-web" data-env="prod"
        data-endpoint="https://galileo-api.serious-sam.dev/otlp"
        data-propagate="https://api.shop.example"></script>
```

`data-propagate` lists the API origins whose requests should carry `traceparent`. For cross-origin
calls allow the header in `config/cors.php`:

```php
'allowed_headers' => ['*'],   // or add 'traceparent', 'tracestate' to your explicit list
```

The Browser page then shows each session with page loads, Web Vitals and JS errors, and every API
call links to its backend trace.

## 10. Deploy markers from CI

After a successful deploy, with the **deploy**-scoped key and the project id (Settings → Project):

```bash
curl -fsS -X POST "https://galileo-api.serious-sam.dev/api/projects/$GALILEO_PROJECT/deploys" \
  -H "Authorization: Bearer $GALILEO_DEPLOY_KEY" -H 'content-type: application/json' \
  -d "{\"version\":\"$GALILEO_RELEASE\",\"service\":\"shop-api\",\"note\":\"$(git log -1 --pretty=%s)\"}"
```

Use the same string as `GALILEO_RELEASE`: charts get a marker and Issues shows the release where
each error first appeared (and flags regressions).

## 11. Verify

1. `curl -I https://api.shop.example/api/health` → response has `x-galileo-trace-id`.
2. Galileo → **Overview**: requests for `shop-api` within a minute.
3. **Traces** → open a `GET /api/orders/{order}` trace: the SQL spans show
   `App\Http\Controllers\OrderController::show` and the file:line.
4. **Logs**: lines carry `user.id` and `tenant.id`; "Open trace" jumps to the request.
5. **Issues**: trigger an exception (e.g. a route that throws) and see it grouped with the user.
6. **AI → Usage**: a `shop-ticket-summary` call with tokens and cost, inside the request trace.
7. Settings → **Usage**: ingest volume and cardinality per signal.

## Troubleshooting

| Symptom | Cause |
| --- | --- |
| Nothing arrives, no errors | `GALILEO_API_KEY` not visible to PHP: config cached without `config/galileo.php`, or FPM `clear_env` dropped it. Check with `php artisan tinker` → `config('galileo.api_key')`. |
| Smoke test returns 404 | Endpoint without `/otlp` on the hosted API (`…/otlp`, not the bare host). |
| 401 / 403 | Wrong key / key without the `ingest` (or `gateway`, `deploy`) scope. |
| 413 | Batch over 32 MB (2 MB on servers older than the fix of 2026-09-23). Usually one huge custom attribute; SQL statements are already capped at 4,000 characters. |
| 429 on ingest | Project's daily quota or ingest queue full; Settings → Project → Quotas / sampling. |
| Requests appear but no SQL spans | The route is not in the `web`/`api` middleware groups, or queries run before the provider boots. |
| No `user.id` | Auth resolved by a guard the default `Auth::user()` does not see — use the identity middleware above. |
| Browser and API traces not linked | `traceparent` not allowed by CORS, or origin missing from `data-propagate`. |
| p95 went up after enabling | Spans are exported at the end of each FPM request. Keep Galileo reachable with low latency (same region), or sample at the project level (Settings → Project → Sampling). |

## Without Laravel

Plain PHP (Slim, Symfony, legacy code) uses the same SDK directly:

```php
use Galileo\Galileo;
use Galileo\Identity;
use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Trace\SpanKind;

Galileo::init([
    'endpoint' => 'https://galileo-api.serious-sam.dev/otlp',
    'api_key'  => getenv('GALILEO_API_KEY'),
    'service'  => 'legacy-shop',
    'env'      => 'prod',
    'version'  => '2026.09.23-1',
]);

// front controller: one server span per request, continuing the caller's trace
$parent = Globals::propagator()->extract(['traceparent' => $_SERVER['HTTP_TRACEPARENT'] ?? '']);
$span = Galileo::tracer()->spanBuilder($_SERVER['REQUEST_METHOD'] . ' ' . $route)
    ->setSpanKind(SpanKind::KIND_SERVER)->setParent($parent)->startSpan();
$scope = $span->storeInContext($parent)->activate();
try {
    Identity::set($user?->id, null, null, $user?->clinicId);
    $response = $app->handle();                          // your code
    $span->setAttribute('http.response.status_code', $response->status);
} catch (Throwable $e) {
    Galileo::exception($e);
    throw $e;
} finally {
    $scope->detach();
    $span->end();
    Identity::clear();
}

// in your DB wrapper, after each query
Galileo::query($sql, $elapsedMs, 'postgresql', 'shop', $bindings);
```

Monolog, custom spans, outgoing `traceparent`, the gateway and deploy markers work exactly as in
the Laravel sections above.
