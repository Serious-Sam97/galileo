# PHP / Laravel SDK (`galileo/php`)

See [sdk/php/README.md](../sdk/php/README.md). Laravel: set `GALILEO_ENDPOINT`, `GALILEO_API_KEY`,
`OTEL_SERVICE_NAME` and the auto-discovered service provider adds request spans named by route, SQL
spans from `DB::listen` with the calling function, identity from `Auth::user()`, exception spans from
the handler and a Monolog handler. Plain PHP: `Galileo::init()`, `Galileo::span()`, `Galileo::query()`,
`Galileo::exception()`, `Identity::set()`.

Worked end-to-end example: [example-php-laravel.md](example-php-laravel.md).
