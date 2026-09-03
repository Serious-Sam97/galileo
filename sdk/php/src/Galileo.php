<?php
declare(strict_types=1);

namespace Galileo;

use OpenTelemetry\API\Globals;
use OpenTelemetry\API\Trace\SpanKind;
use OpenTelemetry\API\Trace\StatusCode;
use OpenTelemetry\API\Trace\TracerInterface;
use OpenTelemetry\Contrib\Otlp\LogsExporter;
use OpenTelemetry\Contrib\Otlp\OtlpHttpTransportFactory;
use OpenTelemetry\Contrib\Otlp\SpanExporter;
use OpenTelemetry\SDK\Common\Attribute\Attributes;
use OpenTelemetry\SDK\Logs\LoggerProvider;
use OpenTelemetry\SDK\Logs\Processor\BatchLogRecordProcessor;
use OpenTelemetry\SDK\Resource\ResourceInfo;
use OpenTelemetry\SDK\Resource\ResourceInfoFactory;
use OpenTelemetry\SDK\Sdk;
use OpenTelemetry\SDK\Trace\SpanProcessor\BatchSpanProcessor;
use OpenTelemetry\SDK\Trace\TracerProvider;
use Throwable;

/**
 * Galileo::init([...]) once at boot; then Galileo::span(), Galileo::query(), Galileo::exception().
 * Env fallbacks: GALILEO_ENDPOINT, GALILEO_API_KEY, OTEL_SERVICE_NAME, GALILEO_ENV, GALILEO_RELEASE.
 */
final class Galileo
{
    private static ?TracerInterface $tracer = null;
    private static ?LoggerProvider $loggerProvider = null;
    private static bool $callSites = true;

    /** @param array{endpoint?: string, api_key?: string, service?: string, env?: string, version?: string, call_sites?: bool} $o */
    public static function init(array $o = []): void
    {
        if (self::$tracer) return;
        $endpoint = rtrim($o['endpoint'] ?? getenv('GALILEO_ENDPOINT') ?: getenv('OTEL_EXPORTER_OTLP_ENDPOINT') ?: 'http://localhost:4318', '/');
        $key = $o['api_key'] ?? (getenv('GALILEO_API_KEY') ?: '');
        $headers = $key !== '' ? ['Authorization' => 'Bearer ' . $key] : [];
        self::$callSites = $o['call_sites'] ?? true;
        $attrs = ['service.name' => $o['service'] ?? (getenv('OTEL_SERVICE_NAME') ?: 'php-app'), 'telemetry.sdk.name' => 'galileo-php'];
        if ($v = $o['version'] ?? getenv('GALILEO_RELEASE')) $attrs['service.version'] = $v;
        if ($e = $o['env'] ?? getenv('GALILEO_ENV')) $attrs['deployment.environment'] = $e;
        $resource = ResourceInfoFactory::defaultResource()->merge(ResourceInfo::create(Attributes::create($attrs)));
        $tf = new OtlpHttpTransportFactory();
        $tracerProvider = TracerProvider::builder()->setResource($resource)
            ->addSpanProcessor(new BatchSpanProcessor(new SpanExporter($tf->create($endpoint . "/v1/traces", "application/x-protobuf", $headers)), \OpenTelemetry\SDK\Common\Time\ClockFactory::getDefault()))
            ->build();
        self::$loggerProvider = LoggerProvider::builder()->setResource($resource)
            ->addLogRecordProcessor(new BatchLogRecordProcessor(new LogsExporter($tf->create($endpoint . '/v1/logs', 'application/x-protobuf', $headers)), \OpenTelemetry\SDK\Common\Time\ClockFactory::getDefault()))
            ->build();
        Sdk::builder()->setTracerProvider($tracerProvider)->setLoggerProvider(self::$loggerProvider)->setAutoShutdown(true)->buildAndRegisterGlobal();
        self::$tracer = $tracerProvider->getTracer('galileo-php', '0.1.0');
    }

    public static function tracer(): TracerInterface
    {
        if (!self::$tracer) self::init();
        return self::$tracer;
    }

    public static function loggerProvider(): ?LoggerProvider { return self::$loggerProvider; }

    /** Run $fn inside a span; identity and (for client spans) call site are attached. */
    public static function span(string $name, callable $fn, int $kind = SpanKind::KIND_INTERNAL, array $attributes = []): mixed
    {
        $span = self::tracer()->spanBuilder($name)->setSpanKind($kind)->startSpan();
        $span->setAttributes($attributes + Identity::attributes());
        if (self::$callSites && $kind === SpanKind::KIND_CLIENT) $span->setAttributes(CallSite::capture());
        $scope = $span->activate();
        try {
            return $fn($span);
        } catch (Throwable $e) {
            $span->recordException($e);
            $span->setStatus(StatusCode::STATUS_ERROR, $e->getMessage());
            $span->setAttributes(['exception.type' => $e::class, 'exception.message' => $e->getMessage(), 'exception.stacktrace' => $e->getTraceAsString()]);
            throw $e;
        } finally {
            $scope->detach();
            $span->end();
        }
    }

    /** A DB query span from (sql, duration) — used by the Laravel DB::listen hook. */
    public static function query(string $sql, float $durationMs, string $system = 'mysql', ?string $connection = null, array $bindings = []): void
    {
        $op = strtoupper(strtok(ltrim($sql), " \t\n") ?: 'QUERY');
        $table = self::tableOf($sql);
        $end = (int) (microtime(true) * 1e9);
        $start = $end - (int) ($durationMs * 1e6);
        $span = self::tracer()->spanBuilder(trim($op . ' ' . $table))->setSpanKind(SpanKind::KIND_CLIENT)->setStartTimestamp($start)->startSpan();
        $attrs = ['db.system' => $system, 'db.statement' => mb_substr($sql, 0, 4000), 'db.operation' => $op] + Identity::attributes();
        if ($table !== '') $attrs['db.sql.table'] = $table;
        if ($connection) $attrs['db.name'] = $connection;
        if ($bindings && getenv('GALILEO_SQL_PARAMS') === '1') $attrs['db.query.parameters'] = json_encode(array_slice($bindings, 0, 50));
        if (self::$callSites) $attrs += CallSite::capture();
        $span->setAttributes($attrs);
        $span->end($end);
    }

    public static function exception(Throwable $e, array $extra = []): void
    {
        $span = self::tracer()->spanBuilder('exception ' . $e::class)->setSpanKind(SpanKind::KIND_INTERNAL)->startSpan();
        $span->recordException($e);
        $span->setStatus(StatusCode::STATUS_ERROR, $e->getMessage());
        $span->setAttributes(['exception.type' => $e::class, 'exception.message' => $e->getMessage(), 'exception.stacktrace' => $e->getTraceAsString(), 'code.file.path' => $e->getFile(), 'code.line.number' => $e->getLine()] + Identity::attributes() + $extra);
        $span->end();
    }

    public static function tableOf(string $sql): string
    {
        if (preg_match('/\b(?:from|into|update|join)\s+[`"\[]?([A-Za-z0-9_.]+)[`"\]]?/i', $sql, $m)) return $m[1];
        return '';
    }
}
