<?php
declare(strict_types=1);

namespace Galileo\Laravel;

use Closure;
use Galileo\Galileo;
use Galileo\Identity;
use Illuminate\Http\Request;
use OpenTelemetry\API\Trace\SpanKind;
use OpenTelemetry\API\Trace\StatusCode;
use OpenTelemetry\Context\Context;
use OpenTelemetry\API\Globals;

/** One server span per request: `GET /orders/{order}`, http.* attributes, identity, W3C traceparent in. */
final class RequestMiddleware
{
    public function handle(Request $request, Closure $next)
    {
        $carrier = ['traceparent' => $request->header('traceparent', ''), 'tracestate' => $request->header('tracestate', '')];
        $parent = Globals::propagator()->extract(array_filter($carrier));
        $span = Galileo::tracer()->spanBuilder($request->method() . ' ' . $request->path())->setSpanKind(SpanKind::KIND_SERVER)->setParent($parent)->startSpan();
        $scope = $span->storeInContext($parent)->activate();
        try {
            $response = $next($request);
            GalileoServiceProvider::identify();
            $route = $request->route();
            $routeName = $route ? ('/' . ltrim($route->uri(), '/')) : $request->path();
            $span->updateName($request->method() . ' ' . $routeName);
            $span->setAttributes([
                'http.request.method' => $request->method(), 'http.route' => $routeName, 'url.path' => '/' . ltrim($request->path(), '/'),
                'http.response.status_code' => $response->getStatusCode(), 'client.address' => $request->ip() ?? '', 'user_agent.original' => (string) $request->userAgent(),
            ] + Identity::attributes());
            if ($response->getStatusCode() >= 500) $span->setStatus(StatusCode::STATUS_ERROR, 'HTTP ' . $response->getStatusCode());
            $response->headers->set('x-galileo-trace-id', $span->getContext()->getTraceId());
            return $response;
        } catch (\Throwable $e) {
            $span->recordException($e);
            $span->setStatus(StatusCode::STATUS_ERROR, $e->getMessage());
            $span->setAttributes(['exception.type' => $e::class, 'exception.message' => $e->getMessage(), 'exception.stacktrace' => $e->getTraceAsString()]);
            throw $e;
        } finally {
            $scope->detach();
            $span->end();
            Identity::clear();
        }
    }
}
