# galileo/php

OpenTelemetry for PHP with the Galileo extras. Laravel is wired automatically; plain PHP works with
`Galileo::init()`.

```bash
composer require galileo/php
```

**Laravel** — add to `.env` and you are done (the service provider is auto-discovered):

```
GALILEO_ENDPOINT=http://galileo:4318
GALILEO_API_KEY=glk_...
OTEL_SERVICE_NAME=shop-api
GALILEO_SQL_PARAMS=1   # optional: record bound parameters on query spans
```

You get one server span per request named by route (`GET /orders/{order}`), a client span per SQL
query with `db.system`, `db.statement`, `db.sql.table` and the calling function (`code.function.name`,
`code.namespace`, `code.file.path`, `code.line.number`, found by skipping `vendor/` and the framework),
`user.id` / `user.email` / `user.name` / `tenant.id` from `Auth::user()` on every span and log,
exception spans from the exception handler, incoming `traceparent` honoured (so `galileo-rum` in the
browser links to the request), and `x-galileo-trace-id` on responses. Monolog: add
`Galileo\MonologHandler` to a channel to ship logs with trace ids.

**Plain PHP**

```php
Galileo::init(['endpoint' => 'http://galileo:4318', 'api_key' => 'glk_...', 'service' => 'billing']);
Identity::set($user->id, $user->email, $user->name, $tenantId);
$total = Galileo::span('price cart', fn () => priceCart($items));
Galileo::query($sql, $ms, 'postgresql');            // from your DB wrapper
Galileo::exception($e);
```

Tests: `composer install && composer test`.
