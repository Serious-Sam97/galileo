"""Redaction of values that may carry personal data before they become attributes."""
import re

_SECRET_KEY = re.compile(r'(password|passwd|secret|token|authorization|api[_-]?key|cookie|cpf|cnpj|card)', re.I)
_EMAIL = re.compile(r'[\w.+-]+@[\w-]+\.[\w.]+')
_CARD = re.compile(r'\b\d(?:[ -]?\d){12,15}\b')
_BEARER = re.compile(r'(?i)bearer\s+[a-z0-9\-._~+/]+=*')


def redact_text(s: str) -> str:
    s = _BEARER.sub('Bearer [REDACTED]', s)
    s = _EMAIL.sub('[EMAIL]', s)
    s = _CARD.sub('[CARD]', s)
    return s


def redact_value(v, limit=200):
    if v is None or isinstance(v, bool):
        return v
    if isinstance(v, (int, float)):
        return v
    s = str(v)
    if len(s) > limit:
        s = s[:limit] + '…'
    return redact_text(s)


def redact_params(params):
    if isinstance(params, dict):
        return str({k: ('[REDACTED]' if _SECRET_KEY.search(str(k)) else redact_value(v)) for k, v in params.items()})
    try:
        return str([redact_value(v) for v in params])
    except TypeError:
        return redact_value(params)


def redact_headers(headers: dict, allow: list) -> dict:
    out = {}
    for name in allow:
        value = headers.get(name)
        if value is None:
            continue
        out[name.lower()] = '[REDACTED]' if _SECRET_KEY.search(name) else redact_value(value, 300)
    return out
