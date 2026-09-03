"""Ship stdlib logging to Galileo with trace ids and identity."""
from __future__ import annotations

import logging

from opentelemetry import trace
from opentelemetry.sdk._logs import LoggingHandler

from ._identity import identity_attributes


class GalileoLogFilter(logging.Filter):
    def filter(self, record):
        ctx = trace.get_current_span().get_span_context()
        if ctx and ctx.is_valid:
            record.trace_id = format(ctx.trace_id, '032x')
            record.span_id = format(ctx.span_id, '016x')
        for k, v in identity_attributes().items():
            setattr(record, k.replace('.', '_'), v)
        return True


class GalileoLoggingHandler(LoggingHandler):
    """LoggingHandler that also carries identity attributes on every record."""
    def _translate(self, record):  # type: ignore[override]
        lr = super()._translate(record)
        attrs = dict(lr.attributes or {})
        attrs.update(identity_attributes())
        lr.attributes = attrs
        return lr
