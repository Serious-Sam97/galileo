"""FastAPI / Starlette: one server span per request named by the route template, identity from a resolver."""
from __future__ import annotations

import time
from typing import Callable, Optional

from opentelemetry import trace
from opentelemetry.propagate import extract
from opentelemetry.trace import SpanKind, Status, StatusCode

from ._identity import clear_identity, set_identity


class GalileoMiddleware:
    """`app.add_middleware(GalileoMiddleware, identify=lambda scope: {...})`. Pure ASGI, no Starlette dependency."""

    def __init__(self, app, identify: Optional[Callable] = None, service_prefix: str = ''):
        self.app = app
        self.identify = identify
        self.prefix = service_prefix

    async def __call__(self, scope, receive, send):
        if scope.get('type') != 'http':
            return await self.app(scope, receive, send)
        headers = {k.decode().lower(): v.decode() for k, v in scope.get('headers', [])}
        ctx = extract(headers)
        method = scope.get('method', 'GET')
        path = scope.get('path', '/')
        tracer = trace.get_tracer('galileo')
        status = {'code': 0}
        start = time.time()
        with tracer.start_as_current_span(f'{method} {path}', context=ctx, kind=SpanKind.SERVER) as span:
            span.set_attributes({'http.request.method': method, 'url.path': path, 'url.scheme': scope.get('scheme', 'http'), 'client.address': (scope.get('client') or ('', 0))[0], 'user_agent.original': headers.get('user-agent', '')})
            if 'x-request-id' in headers: span.set_attribute('request.id', headers['x-request-id'])

            async def send_wrapper(message):
                if message['type'] == 'http.response.start':
                    status['code'] = message['status']
                    hdrs = list(message.get('headers', []))
                    hdrs.append((b'x-galileo-trace-id', format(span.get_span_context().trace_id, '032x').encode()))
                    message['headers'] = hdrs
                await send(message)
            try:
                await self.app(scope, receive, send_wrapper)
            except Exception as e:
                span.record_exception(e)
                span.set_status(Status(StatusCode.ERROR, str(e)[:300]))
                span.set_attributes({'exception.type': type(e).__name__, 'exception.message': str(e)[:2000], 'http.response.status_code': 500})
                clear_identity()
                raise
            # after the app ran, the route template and the user are known
            route = scope.get('route')
            template = getattr(route, 'path', None) or getattr(route, 'path_format', None)
            if template:
                span.update_name(f'{method} {self.prefix}{template}')
                span.set_attribute('http.route', f'{self.prefix}{template}')
            ident = None
            if self.identify:
                try: ident = self.identify(scope)
                except Exception: ident = None
            elif getattr(scope.get('state', None), 'user', None) is not None:
                u = scope['state'].user
                ident = {'user_id': getattr(u, 'id', None), 'email': getattr(u, 'email', None)}
            if ident:
                set_identity(**ident)
                for k, v in __import__('galileo').identity_attributes().items(): span.set_attribute(k, v)
            span.set_attribute('http.response.status_code', status['code'])
            span.set_attribute('http.server.duration_ms', round((time.time() - start) * 1000, 2))
            if status['code'] >= 500:
                span.set_status(Status(StatusCode.ERROR, f'HTTP {status["code"]}'))
            clear_identity()
