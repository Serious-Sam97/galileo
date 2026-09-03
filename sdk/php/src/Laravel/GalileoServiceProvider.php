<?php
declare(strict_types=1);

namespace Galileo\Laravel;

use Galileo\Galileo;
use Galileo\Identity;
use Illuminate\Contracts\Debug\ExceptionHandler;
use Illuminate\Database\Events\QueryExecuted;
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\ServiceProvider;
use Throwable;

/**
 * Registered automatically (composer extra). Set GALILEO_ENDPOINT / GALILEO_API_KEY / OTEL_SERVICE_NAME.
 * Adds: one server span per request (named by route), a client span per SQL query with the calling
 * function (code.*), user identity from Auth on everything, and exception spans from the handler.
 */
final class GalileoServiceProvider extends ServiceProvider
{
    public function register(): void
    {
        if (!getenv('GALILEO_API_KEY') && !config('galileo.api_key')) return;
        Galileo::init(['endpoint' => config('galileo.endpoint') ?: null, 'api_key' => config('galileo.api_key') ?: null, 'service' => config('galileo.service') ?: (getenv('OTEL_SERVICE_NAME') ?: config('app.name'))]);
    }

    public function boot(): void
    {
        if (!getenv('GALILEO_API_KEY') && !config('galileo.api_key')) return;
        $this->app['router']->pushMiddlewareToGroup('web', RequestMiddleware::class);
        $this->app['router']->pushMiddlewareToGroup('api', RequestMiddleware::class);
        DB::listen(function (QueryExecuted $q): void {
            $driver = $q->connection->getDriverName();
            Galileo::query($q->sql, (float) $q->time, $driver === 'pgsql' ? 'postgresql' : $driver, $q->connectionName, $q->bindings);
        });
        $handler = $this->app->make(ExceptionHandler::class);
        if (method_exists($handler, 'reportable')) {
            $handler->reportable(function (Throwable $e): void { Galileo::exception($e); });
        }
    }

    /** Called by the middleware once the user is resolved. */
    public static function identify(): void
    {
        $u = Auth::user();
        if ($u) Identity::set($u->getAuthIdentifier(), $u->email ?? null, $u->name ?? null, $u->tenant_id ?? null);
    }
}
