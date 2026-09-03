"""Agent runs: group the gateway calls of one conversation and mark tool steps.

    from galileo_django import agent

    with agent.run("triage-bot", user=request.user.id) as run:
        answer = groq.chat(messages, headers=run.headers())   # sends x-galileo-conversation-id
        with agent.step("lookup_patient"):
            patient = Patient.objects.get(pk=pk)

`run.headers()` returns the headers a gateway call must carry so Galileo groups it into the run
(conversation id, user id). Steps become spans named `agent.step <name>` in the same trace, so
the run page shows model → tool → model.
"""
from __future__ import annotations

import contextlib
import contextvars
import uuid

from opentelemetry import trace

_current: contextvars.ContextVar["Run | None"] = contextvars.ContextVar("galileo_agent_run", default=None)


class Run:
    def __init__(self, name: str, user=None, conversation_id: str | None = None, tenant=None):
        self.name = name
        self.user = None if user is None else str(user)
        self.tenant = None if tenant is None else str(tenant)
        self.conversation_id = conversation_id or uuid.uuid4().hex

    def headers(self) -> dict:
        h = {"x-galileo-conversation-id": self.conversation_id}
        if self.user:
            h["x-galileo-user-id"] = self.user
        if self.tenant:
            h["x-galileo-tenant-id"] = self.tenant
        return h


def current() -> Run | None:
    return _current.get()


def conversation_headers() -> dict:
    """Headers for the active run, or {} — convenient inside provider code."""
    r = _current.get()
    return r.headers() if r else {}


@contextlib.contextmanager
def run(name: str, user=None, conversation_id: str | None = None, tenant=None):
    r = Run(name, user=user, conversation_id=conversation_id, tenant=tenant)
    token = _current.set(r)
    tracer = trace.get_tracer("galileo-django")
    with tracer.start_as_current_span(f"agent.run {name}") as span:
        span.set_attribute("gen_ai.conversation.id", r.conversation_id)
        span.set_attribute("agent.name", name)
        if r.user:
            span.set_attribute("user.id", r.user)
        try:
            yield r
        finally:
            _current.reset(token)


@contextlib.contextmanager
def step(name: str, **attrs):
    tracer = trace.get_tracer("galileo-django")
    r = _current.get()
    with tracer.start_as_current_span(f"agent.step {name}") as span:
        span.set_attribute("agent.step", name)
        if r:
            span.set_attribute("gen_ai.conversation.id", r.conversation_id)
        for k, v in attrs.items():
            span.set_attribute(k, v)
        yield span
