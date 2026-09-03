"""galileo-python — point any Python service at Galileo.

    import galileo
    galileo.init(endpoint="http://galileo:4318", api_key="glk_...", service="billing")

Then: `galileo.set_identity(user_id=..., tenant=...)`, `@galileo.traced`, `galileo.capture_exception(e)`,
`galileo.gateway_headers()` for gateway calls, and the framework hooks in `galileo.fastapi`,
`galileo.flask`, `galileo.sqlalchemy`, `galileo.celery`. Env fallbacks: GALILEO_ENDPOINT,
GALILEO_API_KEY, OTEL_SERVICE_NAME, GALILEO_ENV, GALILEO_RELEASE.
"""
from __future__ import annotations

import functools
import logging
import os
import traceback
from typing import Optional

from opentelemetry import trace
from opentelemetry.exporter.otlp.proto.http.trace_exporter import OTLPSpanExporter
from opentelemetry.exporter.otlp.proto.http._log_exporter import OTLPLogExporter
from opentelemetry.sdk._logs import LoggerProvider
from opentelemetry.sdk._logs.export import BatchLogRecordProcessor
from opentelemetry.sdk.resources import Resource
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import BatchSpanProcessor
from opentelemetry.trace import SpanKind, Status, StatusCode
from opentelemetry._logs import set_logger_provider

from ._identity import set_identity, clear_identity, identity_attributes
from ._frames import app_frame, code_attributes, EXTRA_SKIP
from ._processor import GalileoProcessor
from ._logs import GalileoLogFilter, GalileoLoggingHandler
from . import agent

__all__ = ['init', 'shutdown', 'set_identity', 'clear_identity', 'identity_attributes', 'traced', 'capture_exception', 'gateway_headers', 'agent', 'tracer']

_state: dict = {'tracer_provider': None, 'logger_provider': None, 'settings': {}}


def init(endpoint: Optional[str] = None, api_key: Optional[str] = None, service: Optional[str] = None, env: Optional[str] = None,
         release: Optional[str] = None, base_dir: Optional[str] = None, call_sites: bool = True, logging_level=logging.INFO,
         skip_frames: Optional[list] = None, resource_attributes: Optional[dict] = None, span_exporter=None, log_exporter=None) -> None:
    """Configure OpenTelemetry for Galileo. Idempotent."""
    if _state['tracer_provider'] is not None:
        return
    endpoint = (endpoint or os.environ.get('GALILEO_ENDPOINT') or os.environ.get('OTEL_EXPORTER_OTLP_ENDPOINT') or 'http://localhost:4318').rstrip('/')
    api_key = api_key or os.environ.get('GALILEO_API_KEY', '')
    headers = {'Authorization': f'Bearer {api_key}'} if api_key else {}
    attrs = {'service.name': service or os.environ.get('OTEL_SERVICE_NAME', 'python-app'), 'telemetry.sdk.name': 'galileo-python'}
    if env or os.environ.get('GALILEO_ENV'): attrs['deployment.environment'] = env or os.environ['GALILEO_ENV']
    if release or os.environ.get('GALILEO_RELEASE'): attrs['service.version'] = release or os.environ['GALILEO_RELEASE']
    attrs.update(resource_attributes or {})
    resource = Resource.create(attrs)
    if skip_frames: EXTRA_SKIP.extend(skip_frames)

    tp = TracerProvider(resource=resource)
    tp.add_span_processor(GalileoProcessor(base_dir=base_dir or os.getcwd(), call_sites=call_sites))
    tp.add_span_processor(BatchSpanProcessor(span_exporter or OTLPSpanExporter(endpoint=f'{endpoint}/v1/traces', headers=headers)))
    trace.set_tracer_provider(tp)
    lp = LoggerProvider(resource=resource)
    lp.add_log_record_processor(BatchLogRecordProcessor(log_exporter or OTLPLogExporter(endpoint=f'{endpoint}/v1/logs', headers=headers)))
    set_logger_provider(lp)
    handler = GalileoLoggingHandler(level=logging_level, logger_provider=lp)
    handler.addFilter(GalileoLogFilter())
    root = logging.getLogger()
    root.addHandler(handler)
    # the root logger drops INFO by default; lower it so app loggers reach the handler
    if root.level == logging.NOTSET or root.level > logging_level:
        root.setLevel(logging_level)
    _state.update(tracer_provider=tp, logger_provider=lp, settings={'endpoint': endpoint, 'service': attrs['service.name'], 'base_dir': base_dir or os.getcwd()})


def shutdown() -> None:
    for k in ('tracer_provider', 'logger_provider'):
        p = _state.get(k)
        if p is not None:
            try: p.shutdown()
            except Exception: pass
            _state[k] = None


def tracer():
    return trace.get_tracer('galileo')


def traced(fn=None, *, name=None, kind=SpanKind.INTERNAL, capture_args=()):
    """Decorator: a span named after the function, with code.* and identity; re-raises."""
    def decorate(func):
        span_name = name or func.__qualname__
        code_attrs = {'code.function.name': func.__name__, 'code.namespace': func.__module__}
        if hasattr(func, '__code__'):
            code_attrs['code.file.path'] = func.__code__.co_filename
            code_attrs['code.line.number'] = func.__code__.co_firstlineno

        def _attrs(args, kwargs):
            a = dict(code_attrs)
            for i, n in enumerate(capture_args):
                if n in kwargs: a[f'arg.{n}'] = str(kwargs[n])[:200]
            return a

        if _is_coroutine(func):
            @functools.wraps(func)
            async def aw(*args, **kwargs):
                with tracer().start_as_current_span(span_name, kind=kind, attributes=_attrs(args, kwargs)) as span:
                    try:
                        return await func(*args, **kwargs)
                    except Exception as e:
                        _record(span, e); raise
            return aw

        @functools.wraps(func)
        def w(*args, **kwargs):
            with tracer().start_as_current_span(span_name, kind=kind, attributes=_attrs(args, kwargs)) as span:
                try:
                    return func(*args, **kwargs)
                except Exception as e:
                    _record(span, e); raise
        return w
    return decorate(fn) if fn else decorate


def _is_coroutine(func):
    import inspect
    return inspect.iscoroutinefunction(func)


def _record(span, e: BaseException):
    span.record_exception(e)
    span.set_status(Status(StatusCode.ERROR, str(e)[:300]))
    span.set_attributes({'exception.type': type(e).__name__, 'exception.message': str(e)[:2000], 'exception.stacktrace': ''.join(traceback.format_exception(type(e), e, e.__traceback__))[:8000]})


def capture_exception(e: BaseException, **extra) -> None:
    """Record an exception on the active span (or a fresh error span) so Issues can group it."""
    span = trace.get_current_span()
    if span is None or not span.get_span_context().is_valid:
        with tracer().start_as_current_span(f'exception {type(e).__name__}') as s:
            _record(s, e); s.set_attributes({k: str(v) for k, v in extra.items()})
    else:
        _record(span, e); span.set_attributes({k: str(v) for k, v in extra.items()})
    logging.getLogger('galileo').error('%s: %s', type(e).__name__, e, extra={'exception.type': type(e).__name__, 'exception.message': str(e)[:2000]})


def gateway_headers() -> dict:
    """Headers for a Galileo gateway call: traceparent + identity + agent conversation."""
    from opentelemetry.propagate import inject
    h: dict = {}
    inject(h)
    ident = identity_attributes()
    if 'user.id' in ident: h['x-galileo-user-id'] = ident['user.id']
    if 'tenant.id' in ident: h['x-galileo-tenant-id'] = ident['tenant.id']
    h.update(agent.conversation_headers())
    return h
