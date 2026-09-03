"""Unhandled exceptions (even from middleware) land on the request span with their traceback."""
import sys

from django.core.signals import got_request_exception
from opentelemetry import trace
from opentelemetry.trace import Status, StatusCode

from .middleware import clean_route, stamp_identity


def on_request_exception(sender, request=None, **kwargs):
    span = trace.get_current_span()
    if not span.is_recording() or request is None:
        return
    exc = sys.exc_info()[1]
    if exc is not None:
        span.record_exception(exc)
        span.set_status(Status(StatusCode.ERROR, f'{type(exc).__name__}: {exc}'[:200]))
    route = clean_route(request)
    if route:
        span.set_attribute('http.route', route)
        span.update_name(f'{request.method} {route}')
    stamp_identity(span, request)


def install():
    got_request_exception.connect(on_request_exception, dispatch_uid='galileo-exception')
