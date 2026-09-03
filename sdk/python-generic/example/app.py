"""FastAPI + SQLAlchemy example. GALILEO_ENDPOINT / GALILEO_API_KEY / OTEL_SERVICE_NAME, then `uvicorn app:app --port 8011`."""
import logging
import galileo
galileo.init()
from fastapi import FastAPI, Request
from galileo.fastapi import GalileoMiddleware
from galileo.sqlalchemy import instrument
from sqlalchemy import create_engine, text

engine = create_engine('sqlite:///example.db')
instrument(engine)
with engine.begin() as c:
    c.execute(text('CREATE TABLE IF NOT EXISTS pets (id INTEGER PRIMARY KEY, name TEXT)'))
    c.execute(text("INSERT INTO pets (name) VALUES ('Rex')"))

app = FastAPI()
app.add_middleware(GalileoMiddleware, identify=lambda scope: {'user_id': dict((k.decode(), v.decode()) for k, v in scope['headers']).get('x-user', 'anon'), 'tenant': 'demo'})
log = logging.getLogger('example')


@galileo.traced
def count_pets():
    with engine.connect() as c:
        return c.execute(text('SELECT count(*) FROM pets')).scalar()


@app.get('/pets')
def pets():
    log.info('listing pets')
    return {'count': count_pets()}


@app.get('/boom')
def boom():
    cart = None
    return cart.total  # AttributeError → 500, captured by the middleware
