import pytest
from opentelemetry.sdk.trace.export import SimpleSpanProcessor
from opentelemetry.sdk.trace.export.in_memory_span_exporter import InMemorySpanExporter

import galileo


def test_fastapi_middleware_names_route_and_identity():
    fastapi = pytest.importorskip('fastapi')
    from fastapi.testclient import TestClient
    from galileo.fastapi import GalileoMiddleware
    ex = InMemorySpanExporter()
    galileo.init(endpoint='http://localhost:1', api_key='x', service='t', span_exporter=ex)
    galileo._state['tracer_provider'].add_span_processor(SimpleSpanProcessor(ex))
    app = fastapi.FastAPI()
    app.add_middleware(GalileoMiddleware, identify=lambda scope: {'user_id': 7, 'tenant': 'clinic'})

    @app.get('/items/{item_id}')
    def read(item_id: int):
        with galileo.tracer().start_as_current_span('SELECT items', kind=__import__('opentelemetry').trace.SpanKind.CLIENT):
            return {'id': item_id}

    r = TestClient(app).get('/items/3', headers={'traceparent': '00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01'})
    assert r.status_code == 200 and r.headers['x-galileo-trace-id'] == '0af7651916cd43dd8448eb211c80319c'
    server = [s for s in ex.get_finished_spans() if s.kind.name == 'SERVER'][-1]
    assert server.name == 'GET /items/{item_id}' and server.attributes['http.route'] == '/items/{item_id}'
    assert server.attributes['user.id'] == '7' and server.attributes['tenant.id'] == 'clinic'
    db = [s for s in ex.get_finished_spans() if s.name == 'SELECT items'][-1]
    assert db.attributes['code.function.name'] == 'read'
