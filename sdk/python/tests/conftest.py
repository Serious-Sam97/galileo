import logging
import os

import pytest

os.environ.setdefault('DJANGO_SETTINGS_MODULE', 'tests.settings')
os.environ['GALILEO_SQL_PARAMS'] = '1'
os.environ['GALILEO_CAPTURE_HEADERS'] = 'user-agent,authorization'

import django  # noqa: E402

django.setup()

from opentelemetry.sdk.trace.export.in_memory_span_exporter import InMemorySpanExporter  # noqa: E402
from opentelemetry.sdk._logs.export import InMemoryLogExporter  # noqa: E402

import galileo_django.config as gconfig  # noqa: E402

_SPANS = InMemorySpanExporter()
_LOGS = InMemoryLogExporter()
gconfig.setup(force=True, exporter=_SPANS, log_exporter=_LOGS)


@pytest.fixture
def spans():
    _SPANS.clear()
    yield _SPANS


@pytest.fixture
def logs():
    _LOGS.clear()
    yield _LOGS


@pytest.fixture
def client(db):
    from django.test import Client
    from tests.models import Pet
    Pet.objects.create(name='Rex')
    Pet.objects.create(name='Mia', species='cat')
    return Client()


def by_name(exporter, needle):
    return [s for s in exporter.get_finished_spans() if needle in s.name]
