"""
Span processor that stamps `code.*` (the application call site) on spans created by libraries:
database queries, outbound HTTP, the gateway. Runs at span start, when the stack is the one
that caused the span.
"""
from opentelemetry.sdk.trace import SpanProcessor

from .frames import app_frame, code_attributes

_LIBRARY_SPAN_HINTS = ('db.system', 'http.url', 'http.method', 'url.full', 'http.request.method')


class CallSiteProcessor(SpanProcessor):
    def __init__(self, base_dir=None):
        self.base_dir = base_dir

    def on_start(self, span, parent_context=None):
        attrs = span.attributes or {}
        if 'code.function.name' in attrs:
            return
        # Only library spans (server spans are named by the middleware, view spans by us).
        if span.kind.name == 'SERVER' or not any(k in attrs for k in _LIBRARY_SPAN_HINTS):
            return
        frame = app_frame()
        if frame is not None:
            span.set_attributes(code_attributes(frame, self.base_dir))

    def on_end(self, span):
        pass

    def shutdown(self):
        pass

    def force_flush(self, timeout_millis=30000):
        return True
