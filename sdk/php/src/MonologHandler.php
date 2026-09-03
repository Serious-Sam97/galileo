<?php
declare(strict_types=1);

namespace Galileo;

use Monolog\Handler\AbstractProcessingHandler;
use Monolog\Level;
use Monolog\LogRecord;
use OpenTelemetry\API\Logs\LogRecord as OtelLogRecord;
use OpenTelemetry\API\Logs\Severity;

/** Sends Monolog records to Galileo as OTLP logs with the active trace id and the current identity. */
final class MonologHandler extends AbstractProcessingHandler
{
    protected function write(LogRecord $record): void
    {
        $lp = Galileo::loggerProvider();
        if (!$lp) return;
        $sev = match (true) {
            $record->level->value >= Level::Critical->value => Severity::FATAL,
            $record->level->value >= Level::Error->value => Severity::ERROR,
            $record->level->value >= Level::Warning->value => Severity::WARN,
            $record->level->value >= Level::Info->value => Severity::INFO,
            default => Severity::DEBUG,
        };
        $attrs = Identity::attributes() + ['log.channel' => $record->channel];
        foreach ($record->context as $k => $v) {
            if ($v instanceof \Throwable) { $attrs['exception.type'] = $v::class; $attrs['exception.message'] = $v->getMessage(); $attrs['exception.stacktrace'] = $v->getTraceAsString(); continue; }
            $attrs[(string) $k] = is_scalar($v) ? $v : json_encode($v);
        }
        $lp->getLogger('galileo-php')->emit((new OtelLogRecord($record->message))->setSeverityNumber($sev)->setSeverityText($record->level->getName())->setAttributes($attrs));
    }
}
