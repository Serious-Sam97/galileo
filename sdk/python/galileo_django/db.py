"""
Database spans via Django's `connection.execute_wrapper`: one span per query with the SQL,
operation, table, row count, parameter count (and, opted in, redacted parameters), and the
application call site. Replaces the psycopg2 instrumentation so all of that lives on one span.
"""
import os
import re
import threading
from contextlib import ExitStack

from django.db import connections
from django.db.backends.signals import connection_created
from opentelemetry import trace
from opentelemetry.trace import SpanKind, Status, StatusCode

from .frames import app_frame, code_attributes
from .redact import redact_params

_tracer = trace.get_tracer('galileo_django.db')
_OP_RE = re.compile(r'^\s*(?:/\*.*?\*/\s*)?(\w+)', re.S)
_TABLE_RE = {
    'SELECT': re.compile(r'\bFROM\s+"?([\w.]+)"?', re.I),
    'INSERT': re.compile(r'\bINTO\s+"?([\w.]+)"?', re.I),
    'UPDATE': re.compile(r'^\s*UPDATE\s+"?([\w.]+)"?', re.I),
    'DELETE': re.compile(r'\bFROM\s+"?([\w.]+)"?', re.I),
}
_local = threading.local()


def parse_sql(sql: str):
    op = (_OP_RE.match(sql or '') or [None, ''])[1].upper()
    table = ''
    rx = _TABLE_RE.get(op)
    if rx:
        m = rx.search(sql)
        if m:
            table = m.group(1)
    return op, table


MAX_STATEMENT = 2048


class _Wrapper:
    def __init__(self, base_dir, capture_params, call_sites=True):
        self.base_dir = base_dir
        self.capture_params = capture_params
        self.call_sites = call_sites

    def __call__(self, execute, sql, params, many, context):
        # A request that sampling dropped (or no provider at all) must cost nothing here: no
        # SQL parsing, no stack walk, no span object. This is what makes GALILEO_SAMPLE_RATIO
        # a real CPU knob rather than only an export knob.
        if not trace.get_current_span().is_recording():
            return execute(sql, params, many, context)
        conn = context.get('connection')
        vendor = getattr(conn, 'vendor', 'sql')
        op, table = parse_sql(sql)
        name = f'{op} {table}' if table else (op or 'query')
        text = sql if len(sql) <= MAX_STATEMENT else sql[:MAX_STATEMENT] + '…'
        attrs = {
            'db.system': {'postgresql': 'postgresql', 'mysql': 'mysql', 'sqlite': 'sqlite'}.get(vendor, vendor),
            'db.query.text': text,
            'db.operation': op,
            'db.table': table,
            'db.name': getattr(conn, 'alias', 'default'),
            'db.params_count': len(params) if params is not None and not many else (len(params) if many and params else 0),
        }
        if many:
            attrs['db.executemany'] = True
        if self.capture_params and params and not many:
            attrs['db.params'] = redact_params(params)
        if self.call_sites:
            frame = app_frame()
            if frame is not None:
                attrs.update(code_attributes(frame, self.base_dir))
        with _tracer.start_as_current_span(name, kind=SpanKind.CLIENT, attributes=attrs) as span:
            try:
                result = execute(sql, params, many, context)
            except Exception as exc:
                span.record_exception(exc)
                span.set_status(Status(StatusCode.ERROR, f'{type(exc).__name__}: {exc}'[:200]))
                raise
            cursor = context.get('cursor')
            rowcount = getattr(cursor, 'rowcount', None)
            if isinstance(rowcount, int) and rowcount >= 0:
                span.set_attribute('db.row_count', rowcount)
            return result


def install(base_dir=None, call_sites=True):
    """Attach the wrapper to every connection, present and future."""
    wrapper = _Wrapper(base_dir, os.environ.get('GALILEO_SQL_PARAMS') == '1', call_sites=call_sites)

    def attach(connection):
        if getattr(connection, '_galileo_wrapped', False):
            return
        connection.execute_wrappers.append(wrapper)
        connection._galileo_wrapped = True

    for conn in connections.all(initialized_only=False):
        attach(conn)
    connection_created.connect(lambda sender, connection, **kw: attach(connection), weak=False, dispatch_uid='galileo-db-wrapper')
    _local.stack = ExitStack()
