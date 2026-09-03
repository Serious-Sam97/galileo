import logging

import pytest
from opentelemetry import trace

from galileo_django import traced, trace_modules
from galileo_django.db import parse_sql
from galileo_django.redact import redact_params, redact_text
from tests.conftest import by_name


def test_parse_sql():
    assert parse_sql('SELECT "a"."id" FROM "tests_pet" WHERE x = %s') == ('SELECT', 'tests_pet')
    assert parse_sql('INSERT INTO tests_pet (name) VALUES (%s)') == ('INSERT', 'tests_pet')
    assert parse_sql('UPDATE "tests_pet" SET name = %s') == ('UPDATE', 'tests_pet')
    assert parse_sql('DELETE FROM tests_pet WHERE id = 1') == ('DELETE', 'tests_pet')
    assert parse_sql('BEGIN') == ('BEGIN', '')


def test_redaction():
    assert redact_text('mail a@b.co card 4111 1111 1111 1111 Bearer abc') == 'mail [EMAIL] card [CARD] Bearer [REDACTED]'
    assert redact_params({'password': 'x', 'name': 'sam@x.io'}) == "{'password': '[REDACTED]', 'name': '[EMAIL]'}"
    assert redact_params([1, 'a' * 300]).endswith("…']")


@pytest.mark.django_db
def test_db_span_has_callsite_and_detail(client, spans):
    r = client.get('/api/pets/?species=dog')
    assert r.status_code == 200
    db = [s for s in spans.get_finished_spans() if s.attributes.get('db.system')]
    sel = [s for s in db if s.attributes.get('db.table') == 'tests_pet' and s.attributes.get('db.operation') == 'SELECT']
    assert sel, [s.name for s in db]
    a = sel[-1].attributes
    assert a['code.function.name'] == 'load_pets'
    assert a['code.namespace'] == 'tests.services'
    assert a['code.file.path'].endswith('tests/services.py')
    assert a['code.line.number'] > 0
    assert a.get('db.row_count', 1) in (1, -1)  # sqlite has no rowcount for SELECT
    assert a['db.params_count'] == 1
    assert "'dog'" in a['db.params']
    assert sel[-1].name == 'SELECT tests_pet'


@pytest.mark.django_db
def test_view_span_and_request_enrichment(client, spans):
    client.get('/api/pets/1/', HTTP_USER_AGENT='pytest-ua', HTTP_AUTHORIZATION='Bearer secret')
    view = by_name(spans, 'tests.views.PetViewSet.retrieve')
    assert len(view) == 1
    va = view[0].attributes
    assert va['code.function.name'] == 'retrieve' and va['django.view.class'] == 'PetViewSet'
    assert va['django.view.args'] == 'pk=1'
    server = [s for s in spans.get_finished_spans() if s.kind.name == 'SERVER'][0]
    sa = server.attributes
    assert sa['http.route'] == '/api/pets/{pk}/'
    assert server.name == 'GET /api/pets/{pk}/'
    assert sa['request.id']
    assert sa['http.request.header.user-agent'] == 'pytest-ua'
    assert sa['http.request.header.authorization'] == '[REDACTED]'
    assert sa['http.response.body.size'] > 0
    # the view span is a child of the server span
    assert view[0].parent.span_id == server.context.span_id


@pytest.mark.django_db
def test_exception_recorded_with_route(client, spans):
    from django.test import Client
    r = Client(raise_request_exception=False).get('/api/pets/13/')
    assert r.status_code == 500
    server = [s for s in spans.get_finished_spans() if s.kind.name == 'SERVER'][0]
    assert server.status.status_code.name == 'ERROR'
    exc = [e for e in server.events if e.name == 'exception']
    assert exc and exc[0].attributes['exception.type'].endswith('RuntimeError')
    assert 'unlucky pet' in exc[0].attributes['exception.stacktrace']
    assert server.attributes['http.route'] == '/api/pets/{pk}/'


def test_traced_decorator(spans):
    from tests.services import Billing

    @traced(capture_args=['invoice_id'])
    def close(invoice_id, secret):
        return Billing().close_invoice(invoice_id)

    assert close(42, secret='s3') == 'closed 42'
    s = by_name(spans, 'close')[0]
    assert s.attributes['code.args'] == 'invoice_id,secret'
    assert s.attributes['code.arg.invoice_id'] == 42
    assert 'code.arg.secret' not in s.attributes
    assert s.attributes['code.return_type'] == 'str'
    assert s.attributes['code.file.path'].endswith('test_sdk.py')

    with pytest.raises(ValueError):
        traced(Billing().explode)()
    err = by_name(spans, 'explode')[0]
    assert err.status.status_code.name == 'ERROR'


def test_trace_modules_wraps_functions_and_methods(spans):
    n = trace_modules(['tests.services'])
    assert n >= 3
    from tests import services
    assert getattr(services.load_pets, '__galileo_traced__', False)
    services.Billing().close_invoice(7)
    assert by_name(spans, 'tests.services.Billing.close_invoice')


@pytest.mark.django_db
def test_log_filter_adds_identity_and_request(client, logs):
    logger = logging.getLogger('myapp')
    client.get('/plain/')  # emits the per-request log line inside the request
    recs = logs.get_finished_logs()
    line = [r for r in recs if 'GET /plain/' in str(r.log_record.body)]
    assert line, [str(r.log_record.body) for r in recs]
    attrs = dict(line[0].log_record.attributes)
    assert attrs['request.id'] and attrs['http.route'] == '/plain/'
    assert attrs['http.status_code'] == 200
    logger.warning('outside request', extra={'order.id': 9})
    outside = [r for r in logs.get_finished_logs() if 'outside request' in str(r.log_record.body)][0]
    assert dict(outside.log_record.attributes)['order.id'] == 9
