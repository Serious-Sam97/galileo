from __future__ import annotations

import contextvars

_identity: contextvars.ContextVar[dict] = contextvars.ContextVar('galileo_identity', default={})


def set_identity(user_id=None, email=None, name=None, tenant=None, **extra) -> None:
    d = {}
    if user_id is not None: d['user.id'] = str(user_id)
    if email: d['user.email'] = email
    if name: d['user.name'] = name
    if tenant is not None: d['tenant.id'] = str(tenant)
    for k, v in extra.items():
        if v is not None: d[k] = str(v)
    _identity.set(d)


def clear_identity() -> None:
    _identity.set({})


def identity_attributes() -> dict:
    return dict(_identity.get())
