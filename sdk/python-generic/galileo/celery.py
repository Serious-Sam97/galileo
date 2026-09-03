"""Celery: a span per task run with identity carried in the task headers."""
from __future__ import annotations

from opentelemetry import trace
from opentelemetry.propagate import extract, inject
from opentelemetry.trace import SpanKind, Status, StatusCode

from ._identity import clear_identity, identity_attributes, set_identity


def instrument():
    from celery import signals

    tracer = trace.get_tracer('galileo')
    spans = {}

    @signals.before_task_publish.connect
    def _publish(headers=None, **kw):
        if headers is not None:
            inject(headers)
            ident = identity_attributes()
            if ident: headers['galileo_identity'] = ident

    @signals.task_prerun.connect
    def _pre(task_id=None, task=None, **kw):
        req = getattr(task, 'request', None)
        headers = dict(getattr(req, 'headers', None) or {})
        ctx = extract(headers)
        span = tracer.start_span(f'celery {task.name}', context=ctx, kind=SpanKind.CONSUMER)
        span.set_attributes({'messaging.system': 'celery', 'celery.task': task.name, 'celery.task_id': str(task_id)})
        ident = headers.get('galileo_identity') or {}
        if ident:
            set_identity(user_id=ident.get('user.id'), email=ident.get('user.email'), tenant=ident.get('tenant.id'))
            span.set_attributes(ident)
        spans[task_id] = (span, trace.use_span(span, end_on_exit=False).__enter__())

    @signals.task_postrun.connect
    def _post(task_id=None, state=None, **kw):
        g = spans.pop(task_id, None)
        if g:
            g[0].set_attribute('celery.state', str(state)); g[0].end()
        clear_identity()

    @signals.task_failure.connect
    def _fail(task_id=None, exception=None, **kw):
        g = spans.get(task_id)
        if g and exception is not None:
            g[0].record_exception(exception); g[0].set_status(Status(StatusCode.ERROR, str(exception)[:300]))
            g[0].set_attributes({'exception.type': type(exception).__name__, 'exception.message': str(exception)[:2000]})
