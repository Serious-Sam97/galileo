"""Span processor: identity on every span, call site on client spans (DB/HTTP)."""
from __future__ import annotations

from opentelemetry.sdk.trace import SpanProcessor
from opentelemetry.trace import SpanKind

from ._frames import app_frame, code_attributes
from ._identity import identity_attributes


class GalileoProcessor(SpanProcessor):
    def __init__(self, base_dir=None, call_sites=True):
        self.base_dir = base_dir
        self.call_sites = call_sites

    def on_start(self, span, parent_context=None):
        attrs = identity_attributes()
        if self.call_sites and span.kind == SpanKind.CLIENT and 'code.function.name' not in (span.attributes or {}):
            f = app_frame()
            if f is not None:
                attrs.update(code_attributes(f, self.base_dir))
        if attrs:
            span.set_attributes(attrs)

    def on_end(self, span): pass
    def shutdown(self): pass
    def force_flush(self, timeout_millis=30000): return True
