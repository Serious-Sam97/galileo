"""Agent runs: group the gateway calls of one conversation; mark tool steps (see galileo_django.agent)."""
from __future__ import annotations

import contextlib
import contextvars
import uuid

from opentelemetry import trace

from ._identity import identity_attributes

_current: contextvars.ContextVar = contextvars.ContextVar('galileo_agent_run', default=None)


class Run:
    def __init__(self, name, conversation_id=None):
        self.name = name
        self.conversation_id = conversation_id or uuid.uuid4().hex

    def headers(self) -> dict:
        h = {'x-galileo-conversation-id': self.conversation_id}
        ident = identity_attributes()
        if 'user.id' in ident: h['x-galileo-user-id'] = ident['user.id']
        if 'tenant.id' in ident: h['x-galileo-tenant-id'] = ident['tenant.id']
        return h


def current():
    return _current.get()


def conversation_headers() -> dict:
    r = _current.get()
    return r.headers() if r else {}


@contextlib.contextmanager
def run(name, conversation_id=None):
    r = Run(name, conversation_id)
    token = _current.set(r)
    with trace.get_tracer('galileo').start_as_current_span(f'agent.run {name}') as span:
        span.set_attribute('gen_ai.conversation.id', r.conversation_id)
        span.set_attribute('agent.name', name)
        try:
            yield r
        finally:
            _current.reset(token)


@contextlib.contextmanager
def step(name, **attrs):
    r = _current.get()
    with trace.get_tracer('galileo').start_as_current_span(f'agent.step {name}') as span:
        span.set_attribute('agent.step', name)
        if r: span.set_attribute('gen_ai.conversation.id', r.conversation_id)
        for k, v in attrs.items(): span.set_attribute(k, v)
        yield span
