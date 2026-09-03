"""Flask: `galileo.flask.instrument(app, identify=lambda: current_user)`."""
from __future__ import annotations

from opentelemetry import trace
from opentelemetry.propagate import extract
from opentelemetry.trace import SpanKind, Status, StatusCode

from ._identity import clear_identity, set_identity


def instrument(app, identify=None):
    from flask import g, request

    tracer = trace.get_tracer('galileo')

    @app.before_request
    def _start():
        ctx = extract(dict(request.headers))
        span = tracer.start_span(f'{request.method} {request.url_rule.rule if request.url_rule else request.path}', context=ctx, kind=SpanKind.SERVER)
        span.set_attributes({'http.request.method': request.method, 'url.path': request.path, 'http.route': request.url_rule.rule if request.url_rule else '', 'client.address': request.remote_addr or '', 'user_agent.original': request.user_agent.string})
        g._galileo_token = trace.use_span(span, end_on_exit=False).__enter__()
        g._galileo_span = span
        if identify:
            try:
                ident = identify()
                if ident: set_identity(**ident)
            except Exception: pass

    @app.after_request
    def _end(resp):
        span = getattr(g, '_galileo_span', None)
        if span:
            span.set_attribute('http.response.status_code', resp.status_code)
            if resp.status_code >= 500: span.set_status(Status(StatusCode.ERROR, f'HTTP {resp.status_code}'))
            resp.headers['x-galileo-trace-id'] = format(span.get_span_context().trace_id, '032x')
        return resp

    @app.teardown_request
    def _teardown(exc):
        span = getattr(g, '_galileo_span', None)
        if span:
            if exc is not None:
                span.record_exception(exc); span.set_status(Status(StatusCode.ERROR, str(exc)[:300]))
                span.set_attributes({'exception.type': type(exc).__name__, 'exception.message': str(exc)[:2000]})
            span.end()
        clear_identity()
