"""
Logging filter: every record emitted during a request carries who and where. `extra={...}`
keys become attributes too (the OpenTelemetry handler already forwards record attributes).
"""
import logging

from opentelemetry import trace

from .context import current_request
from .middleware import clean_route

_STD_KEYS = set(logging.LogRecord('x', 0, 'x', 0, '', (), None).__dict__) | {'message', 'asctime'}


class GalileoLogFilter(logging.Filter):
    def filter(self, record):
        request = current_request()
        if request is not None:
            rid = getattr(request, 'galileo_request_id', None)
            if rid:
                record.__dict__.setdefault('request.id', rid)
            route = clean_route(request)
            if route:
                record.__dict__.setdefault('http.route', route)
            record.__dict__.setdefault('http.method', request.method)
            tenant = getattr(request, 'tenant', None)
            if tenant is not None:
                record.__dict__.setdefault('tenant.id', str(getattr(tenant, 'pk', tenant)))
                dom = getattr(tenant, 'dominio', None) or getattr(tenant, 'domain', None)
                if dom:
                    record.__dict__.setdefault('tenant.domain', str(dom))
            user = getattr(request, 'user', None)
            if user is not None and getattr(user, 'is_authenticated', False):
                record.__dict__.setdefault('user.id', str(user.pk))
        span = trace.get_current_span()
        attrs = getattr(span, 'attributes', None) or {}
        fn = attrs.get('code.function.name')
        if fn and 'code.function.name' not in record.__dict__:
            record.__dict__['code.function.name'] = fn
            if attrs.get('code.namespace'):
                record.__dict__['code.namespace'] = attrs['code.namespace']
        return True


def install():
    root = logging.getLogger()
    if not any(isinstance(f, GalileoLogFilter) for f in root.filters):
        root.addFilter(GalileoLogFilter())
    # Filters on the root logger do not apply to records from child loggers; install on handlers.
    for h in root.handlers:
        if not any(isinstance(f, GalileoLogFilter) for f in h.filters):
            h.addFilter(GalileoLogFilter())
