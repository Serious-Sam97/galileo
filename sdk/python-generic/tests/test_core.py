import logging

import pytest
from opentelemetry.sdk.trace.export import SimpleSpanProcessor
from opentelemetry.sdk.trace.export.in_memory_span_exporter import InMemorySpanExporter
from opentelemetry.sdk._logs.export import InMemoryLogExporter, SimpleLogRecordProcessor

import galileo
from galileo import _processor


@pytest.fixture(scope='module')
def exporter():
    ex = InMemorySpanExporter()
    galileo.init(endpoint='http://localhost:1', api_key='x', service='t', span_exporter=ex, log_exporter=None)
    # swap the batch processor for a simple one so spans are visible immediately
    tp = galileo._state['tracer_provider']
    tp.add_span_processor(SimpleSpanProcessor(ex))
    return ex


def test_traced_and_identity(exporter):
    galileo.set_identity(user_id=42, email='a@b.c', tenant='acme')

    @galileo.traced
    def load_orders(n):
        return n * 2

    assert load_orders(2) == 4
    spans = [s for s in exporter.get_finished_spans() if s.name.endswith('load_orders')]
    assert spans, [s.name for s in exporter.get_finished_spans()]
    a = spans[-1].attributes
    assert a['user.id'] == '42' and a['tenant.id'] == 'acme'
    assert a['code.function.name'] == 'load_orders'


def test_client_span_gets_call_site(exporter):
    from opentelemetry.trace import SpanKind

    def fetch_customer():
        with galileo.tracer().start_as_current_span('SELECT customers', kind=SpanKind.CLIENT):
            pass

    fetch_customer()
    s = [s for s in exporter.get_finished_spans() if s.name == 'SELECT customers'][-1]
    assert s.attributes['code.function.name'] == 'fetch_customer'
    assert s.attributes['code.file.path'].endswith('test_core.py')


def test_capture_exception_sets_status(exporter):
    with galileo.tracer().start_as_current_span('work') as span:
        try:
            raise ValueError('boom')
        except ValueError as e:
            galileo.capture_exception(e)
    s = [s for s in exporter.get_finished_spans() if s.name == 'work'][-1]
    assert s.attributes['exception.type'] == 'ValueError'
    assert s.status.status_code.name == 'ERROR'


def test_gateway_headers_carry_trace_and_identity(exporter):
    galileo.set_identity(user_id='u1', tenant='t1')
    with galileo.tracer().start_as_current_span('req'):
        with galileo.agent.run('bot') as r:
            h = galileo.gateway_headers()
    assert h['traceparent'].startswith('00-')
    assert h['x-galileo-user-id'] == 'u1' and h['x-galileo-tenant-id'] == 't1'
    assert h['x-galileo-conversation-id'] == r.conversation_id


def test_sqlalchemy_parse():
    from galileo.sqlalchemy import parse_sql
    assert parse_sql('select * from "orders" where id = 1') == ('SELECT', 'orders')
    assert parse_sql('UPDATE public.users SET x=1') == ('UPDATE', 'public.users')
