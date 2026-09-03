"""SQLAlchemy: DB spans with statement, table, operation and the calling function.

    galileo.sqlalchemy.instrument(engine, capture_params=False)
"""
from __future__ import annotations

import re
import time

from opentelemetry import trace
from opentelemetry.trace import SpanKind, Status, StatusCode

_TABLE = re.compile(r'\b(?:from|into|update|join)\s+[`"\[]?([A-Za-z0-9_.]+)[`"\]]?', re.I)


def parse_sql(sql: str):
    op = (sql.strip().split(None, 1) or ['QUERY'])[0].upper()
    m = _TABLE.search(sql)
    return op, (m.group(1) if m else '')


def instrument(engine, capture_params: bool = False, system: str | None = None):
    from sqlalchemy import event
    tracer = trace.get_tracer('galileo')
    dialect = system or engine.dialect.name

    @event.listens_for(engine, 'before_cursor_execute')
    def _before(conn, cursor, statement, parameters, context, executemany):
        op, table = parse_sql(statement)
        span = tracer.start_span(f'{op} {table}'.strip(), kind=SpanKind.CLIENT)
        span.set_attributes({'db.system': dialect, 'db.statement': statement[:4000], 'db.operation': op, 'db.sql.table': table, 'db.name': str(engine.url.database or '')})
        if capture_params and parameters:
            span.set_attribute('db.query.parameters', str(parameters)[:1000])
        context._galileo = (span, trace.use_span(span, end_on_exit=False).__enter__(), time.time())

    @event.listens_for(engine, 'after_cursor_execute')
    def _after(conn, cursor, statement, parameters, context, executemany):
        g = getattr(context, '_galileo', None)
        if g:
            span, _, _ = g
            try: span.set_attribute('db.rows_affected', cursor.rowcount if cursor.rowcount is not None else -1)
            except Exception: pass
            span.end()

    @event.listens_for(engine, 'handle_error')
    def _error(ctx):
        c = getattr(ctx, 'execution_context', None)
        g = getattr(c, '_galileo', None) if c else None
        if g:
            span, _, _ = g
            e = ctx.original_exception
            span.record_exception(e); span.set_status(Status(StatusCode.ERROR, str(e)[:300]))
            span.set_attributes({'exception.type': type(e).__name__, 'exception.message': str(e)[:2000]})
            span.end()
