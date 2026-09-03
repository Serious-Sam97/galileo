"""
Two middlewares:

* `GalileoContextMiddleware` — put it FIRST in MIDDLEWARE. Gives the request an id, stamps the
  server span with route/identity/sizes/headers on the way out, records the request metric and
  the per-request log line, and exposes the request to the logging filter.
* `GalileoViewMiddleware` — put it LAST. Wraps the view call in a span named after the view
  (`pets.views.PetViewSet.retrieve`) with code.* attributes, so the view is a node in the tree
  and everything it triggers (queries, HTTP, LLM calls) hangs under it.
"""
import logging
import re
import time
import uuid

from django.urls import Resolver404, resolve
from opentelemetry import trace
from opentelemetry.trace import SpanKind

from . import config
from .context import current_request, set_current_request
from .frames import callable_attributes
from .redact import redact_headers

_log = logging.getLogger('galileo.request')
_tracer = trace.get_tracer('galileo_django.view')
_PARAM_RE = re.compile(r'\(\?P<(\w+)>[^)]*\)')


def clean_route(request):
    """DRF's `^pacientes/(?P<pk>[^/.]+)/$` → `/api/pacientes/{pk}/`."""
    match = getattr(request, 'resolver_match', None)
    if match is None:
        try:
            match = resolve(request.path_info)
        except Resolver404:
            return None
    if not match or not match.route:
        return None
    route = '/' + match.route.lstrip('^').rstrip('$').lstrip('/')
    return _PARAM_RE.sub(r'{\1}', route)


def stamp_identity(span, request):
    tenant = getattr(request, 'tenant', None)
    if tenant is not None:
        span.set_attribute('tenant.id', str(getattr(tenant, 'pk', tenant)))
        for attr in ('dominio', 'domain', 'slug', 'name', 'nome'):
            value = getattr(tenant, attr, None)
            if value:
                span.set_attribute('tenant.domain' if attr in ('dominio', 'domain') else 'tenant.name', str(value))
                break
    user = getattr(request, 'user', None)
    if user is not None and getattr(user, 'is_authenticated', False):
        span.set_attribute('user.id', str(user.pk))
        if config.settings().capture_user_email and getattr(user, 'email', ''):
            span.set_attribute('user.email', user.email)


class GalileoContextMiddleware:
    def __init__(self, get_response):
        self.get_response = get_response

    def __call__(self, request):
        cfg = config.settings()
        request.galileo_request_id = request.headers.get('X-Request-ID') or uuid.uuid4().hex[:16]
        set_current_request(request)
        started = time.perf_counter()
        span = trace.get_current_span()
        if span.is_recording():
            span.set_attribute('request.id', request.galileo_request_id)
        try:
            response = self.get_response(request)
        except BaseException:
            set_current_request(None)
            raise
        elapsed_ms = (time.perf_counter() - started) * 1000
        route = clean_route(request) or request.path
        if span.is_recording():
            span.set_attribute('http.route', route)
            span.update_name(f'{request.method} {route}')
            stamp_identity(span, request)
            span.set_attribute('http.request.body.size', int(request.META.get('CONTENT_LENGTH') or 0))
            if request.content_type:
                span.set_attribute('http.request.content_type', request.content_type)
            if request.GET:
                span.set_attribute('url.query.keys', ','.join(sorted(request.GET.keys()))[:500])
            if hasattr(response, 'content') and not getattr(response, 'streaming', False):
                span.set_attribute('http.response.body.size', len(response.content))
            for k, v in redact_headers(request.headers, cfg.capture_headers).items():
                span.set_attribute(f'http.request.header.{k}', v)
        tenant = getattr(request, 'tenant', None)
        labels = {'http.route': route, 'http.method': request.method, 'http.status_code': response.status_code,
                  'tenant.domain': getattr(tenant, 'dominio', None) or getattr(tenant, 'domain', None) or 'platform'}
        config.record_request(elapsed_ms, labels)
        try:
            _log.log(logging.WARNING if response.status_code >= 500 else logging.INFO,
                     '%s %s -> %s in %.1fms', request.method, request.path, response.status_code, elapsed_ms,
                     extra={'http.status_code': response.status_code, 'http.response.duration_ms': round(elapsed_ms, 1)})
        finally:
            set_current_request(None)
        return response


class GalileoViewMiddleware:
    def __init__(self, get_response):
        self.get_response = get_response

    def __call__(self, request):
        match = getattr(request, 'resolver_match', None)
        if match is None:
            try:
                match = resolve(request.path_info)
            except Resolver404:
                match = None
        if match is None or not trace.get_current_span().is_recording():
            return self.get_response(request)
        view = match.func
        cls = getattr(view, 'cls', None) or getattr(view, 'view_class', None)
        base_dir = config.settings().base_dir
        if cls is not None:
            method = request.method.lower()
            actions = getattr(view, 'actions', None) or {}
            action = actions.get(method, method if hasattr(cls, method) else 'dispatch')
            target = getattr(cls, action, cls)
            attrs = callable_attributes(target, base_dir)
            attrs['code.namespace'] = f'{cls.__module__}.{cls.__name__}'
            attrs['code.function.name'] = action
            name = f'{cls.__module__}.{cls.__name__}.{action}'
            attrs['django.view.class'] = cls.__name__
        else:
            attrs = callable_attributes(view, base_dir)
            name = f"{attrs.get('code.namespace', '')}.{attrs['code.function.name']}".strip('.')
        attrs['django.view.name'] = match.view_name or ''
        attrs['django.view.args'] = ','.join(f'{k}={v}' for k, v in (match.kwargs or {}).items())[:200]
        with _tracer.start_as_current_span(name, kind=SpanKind.INTERNAL, attributes=attrs) as span:
            response = self.get_response(request)
            renderer = getattr(response, 'accepted_renderer', None)
            if renderer is not None:
                span.set_attribute('drf.renderer', type(renderer).__name__)
            tmpl = getattr(response, 'template_name', None)
            if tmpl:
                span.set_attribute('django.template', str(tmpl))
            span.set_attribute('http.status_code', response.status_code)
            return response
