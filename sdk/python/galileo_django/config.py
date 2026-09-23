"""
Setup: exporters, instrumentation, processors. Off unless GALILEO_OTLP_ENDPOINT is set; also
off under pytest and for management commands (GALILEO_FORCE=1 opts a command in).

Env:
  GALILEO_OTLP_ENDPOINT   http://host:4318
  GALILEO_API_KEY         glk_...
  OTEL_SERVICE_NAME       service name (default: the Django project package)
  GALILEO_ENV             deployment.environment.name (default dev)
  GALILEO_TRACE_MODULES   comma list: myapp.services,billing.*
  GALILEO_SQL_PARAMS      1 → redacted SQL parameters on DB spans
  GALILEO_CAPTURE_HEADERS comma list of request headers to record (default: user-agent,x-tenant-id,x-request-id)
  GALILEO_CAPTURE_USER_EMAIL 1 → user.email on spans
  GALILEO_DISABLED / GALILEO_FORCE
  GALILEO_SAMPLE_RATIO    0.0–1.0 share of traces kept (default 1); parent-based, so propagated traces stay whole
  GALILEO_CALL_SITES      0 → skip the per-span stack walk that records code.* (cheapest way to cut CPU)
  GALILEO_LOGS            0 → do not export log records
  GALILEO_METRICS_INTERVAL seconds between metric exports (default 60)
"""
import logging
import os
import sys
from dataclasses import dataclass, field

from opentelemetry import metrics, trace

_state = {'enabled': False, 'settings': None, 'histogram': None, 'counter': None}


@dataclass
class Settings:
    endpoint: str = ''
    api_key: str = ''
    service_name: str = 'django'
    environment: str = 'dev'
    base_dir: str = ''
    trace_modules: list = field(default_factory=list)
    sql_params: bool = False
    capture_headers: list = field(default_factory=lambda: ['user-agent', 'x-tenant-id', 'x-request-id'])
    capture_user_email: bool = False
    sample_ratio: float = 1.0
    call_sites: bool = True
    export_logs: bool = True
    metrics_interval: int = 60


def settings() -> Settings:
    return _state['settings'] or Settings()


def is_enabled() -> bool:
    return _state['enabled']


def _serving() -> bool:
    argv0 = sys.argv[0] if sys.argv else ''
    argv1 = sys.argv[1] if len(sys.argv) > 1 else ''
    return any(s in argv0 for s in ('gunicorn', 'uvicorn', 'daphne', 'hypercorn')) or argv1 in ('runserver', 'runserver_plus')


def _ratio(v: str) -> float:
    try:
        return min(1.0, max(0.0, float(v)))
    except ValueError:
        return 1.0


def _from_env() -> Settings:
    from django.conf import settings as dj
    service = os.environ.get('OTEL_SERVICE_NAME') or os.environ.get('GALILEO_SERVICE_NAME') or (getattr(dj, 'ROOT_URLCONF', 'django').split('.')[0])
    return Settings(
        endpoint=os.environ.get('GALILEO_OTLP_ENDPOINT', '').rstrip('/'),
        api_key=os.environ.get('GALILEO_API_KEY', ''),
        service_name=service,
        environment=os.environ.get('GALILEO_ENV', 'dev'),
        base_dir=str(getattr(dj, 'BASE_DIR', '') or ''),
        trace_modules=[m for m in os.environ.get('GALILEO_TRACE_MODULES', '').split(',') if m.strip()],
        sql_params=os.environ.get('GALILEO_SQL_PARAMS') == '1',
        capture_headers=[h.strip().lower() for h in os.environ.get('GALILEO_CAPTURE_HEADERS', 'user-agent,x-tenant-id,x-request-id').split(',') if h.strip()],
        capture_user_email=os.environ.get('GALILEO_CAPTURE_USER_EMAIL') == '1',
        sample_ratio=_ratio(os.environ.get('GALILEO_SAMPLE_RATIO', '1')),
        call_sites=os.environ.get('GALILEO_CALL_SITES', '1') != '0',
        export_logs=os.environ.get('GALILEO_LOGS', '1') != '0',
        metrics_interval=int(os.environ.get('GALILEO_METRICS_INTERVAL', '60') or 60),
    )


def setup(force: bool = False, exporter=None, log_exporter=None, metric_reader=None):
    """
    Wire everything. `exporter`/`log_exporter`/`metric_reader` exist for tests (in-memory).
    """
    if _state['enabled']:
        return True
    cfg = _from_env()
    _state['settings'] = cfg
    testing = 'pytest' in sys.modules and exporter is None
    if os.environ.get('GALILEO_DISABLED') == '1' or testing:
        return False
    if not force and exporter is None and (not cfg.endpoint or (not _serving() and os.environ.get('GALILEO_FORCE') != '1')):
        return False

    from opentelemetry.sdk.resources import Resource
    from opentelemetry.sdk.trace import TracerProvider
    from opentelemetry.sdk.trace.export import BatchSpanProcessor, SimpleSpanProcessor
    from opentelemetry.sdk._logs import LoggerProvider, LoggingHandler
    from opentelemetry.sdk._logs.export import BatchLogRecordProcessor
    from opentelemetry.sdk.metrics import MeterProvider
    from opentelemetry.sdk.metrics.export import PeriodicExportingMetricReader
    from opentelemetry.instrumentation.django import DjangoInstrumentor
    from opentelemetry.instrumentation.requests import RequestsInstrumentor

    from .callsite import CallSiteProcessor
    from . import db as _db
    from . import logs as _logs
    from .signals import install as _install_signals
    from .tracing import trace_modules

    resource = Resource.create({
        'service.name': cfg.service_name,
        'deployment.environment.name': cfg.environment,
        'service.version': os.environ.get('APP_VERSION', 'dev'),
        'telemetry.distro.name': 'galileo-django',
    })
    headers = {'x-galileo-key': cfg.api_key} if cfg.api_key else {}

    from opentelemetry.sdk.trace.sampling import ParentBased, TraceIdRatioBased, ALWAYS_ON
    sampler = ALWAYS_ON if cfg.sample_ratio >= 1.0 else ParentBased(TraceIdRatioBased(cfg.sample_ratio))
    provider = TracerProvider(resource=resource, sampler=sampler)
    if cfg.call_sites:
        provider.add_span_processor(CallSiteProcessor(cfg.base_dir))
    if exporter is not None:
        provider.add_span_processor(SimpleSpanProcessor(exporter))
    else:
        from opentelemetry.exporter.otlp.proto.http.trace_exporter import OTLPSpanExporter
        provider.add_span_processor(BatchSpanProcessor(OTLPSpanExporter(endpoint=f'{cfg.endpoint}/v1/traces', headers=headers)))
    trace.set_tracer_provider(provider)

    log_provider = LoggerProvider(resource=resource)
    if log_exporter is not None:
        from opentelemetry.sdk._logs.export import SimpleLogRecordProcessor
        log_provider.add_log_record_processor(SimpleLogRecordProcessor(log_exporter))
    elif exporter is None and cfg.export_logs:
        from opentelemetry.exporter.otlp.proto.http._log_exporter import OTLPLogExporter
        log_provider.add_log_record_processor(BatchLogRecordProcessor(OTLPLogExporter(endpoint=f'{cfg.endpoint}/v1/logs', headers=headers)))
    if log_exporter is not None or cfg.export_logs:
        handler = LoggingHandler(level=logging.INFO, logger_provider=log_provider)
        logging.getLogger().addHandler(handler)
    logging.getLogger('galileo').setLevel(logging.INFO)
    logging.getLogger('django.request').setLevel(logging.WARNING)
    _logs.install()

    readers = []
    if metric_reader is not None:
        readers.append(metric_reader)
    elif exporter is None:
        from opentelemetry.exporter.otlp.proto.http.metric_exporter import OTLPMetricExporter
        readers.append(PeriodicExportingMetricReader(OTLPMetricExporter(endpoint=f'{cfg.endpoint}/v1/metrics', headers=headers), export_interval_millis=cfg.metrics_interval * 1000))
    metrics.set_meter_provider(MeterProvider(resource=resource, metric_readers=readers))
    meter = metrics.get_meter('galileo_django')
    _state['histogram'] = meter.create_histogram('http.server.request.duration', unit='ms', description='Request latency')
    _state['counter'] = meter.create_counter('http.server.requests', unit='{request}', description='Requests handled')
    try:
        from opentelemetry.instrumentation.system_metrics import SystemMetricsInstrumentor
        SystemMetricsInstrumentor().instrument()
    except ImportError:
        pass

    DjangoInstrumentor().instrument()
    RequestsInstrumentor().instrument()
    _db.install(cfg.base_dir, call_sites=cfg.call_sites)
    _install_signals()
    if cfg.trace_modules:
        trace_modules(cfg.trace_modules)
    _state['enabled'] = True
    return True


def record_request(elapsed_ms, labels):
    if _state['histogram'] is not None:
        _state['histogram'].record(elapsed_ms, labels)
        _state['counter'].add(1, labels)


def _reset_for_tests():
    _state.update({'enabled': False, 'settings': None, 'histogram': None, 'counter': None})
