"""Which application function is running: first stack frame outside site-packages, the stdlib,
frameworks and this package, as OTel code.* attributes."""
from __future__ import annotations

import inspect
import os
import sys
from types import FrameType
from typing import Optional

_PKG = os.path.dirname(os.path.abspath(__file__))
_STDLIB = os.path.dirname(os.__file__)
_SKIP_PARTS = ('site-packages', 'dist-packages', '/opentelemetry/', '/starlette/', '/fastapi/', '/flask/', '/sqlalchemy/', '/celery/', '/werkzeug/', '/uvicorn/', '/anyio/', '/asyncio/', '<frozen')
EXTRA_SKIP: list[str] = []


def is_app_frame(frame: FrameType) -> bool:
    f = frame.f_code.co_filename
    if not f or f.startswith('<'):
        return False
    if f.startswith(_PKG) or f.startswith(_STDLIB) or f.startswith(sys.prefix):
        return False
    return not any(p in f for p in _SKIP_PARTS) and not any(p in f for p in EXTRA_SKIP)


def app_frame(start: Optional[FrameType] = None, max_depth: int = 60) -> Optional[FrameType]:
    f = start or inspect.currentframe()
    d = 0
    while f is not None and d < max_depth:
        if is_app_frame(f):
            return f
        f = f.f_back
        d += 1
    return None


def code_attributes(frame: FrameType, base_dir: Optional[str] = None) -> dict:
    code = frame.f_code
    path = code.co_filename
    base = base_dir or os.getcwd()
    rel = os.path.relpath(path, base) if path.startswith(base) else path
    self_obj = frame.f_locals.get('self')
    cls = type(self_obj).__name__ if self_obj is not None else frame.f_locals.get('cls').__name__ if inspect.isclass(frame.f_locals.get('cls')) else None
    mod = frame.f_globals.get('__name__', '')
    ns = f'{mod}.{cls}' if cls else mod
    return {'code.function.name': code.co_name, 'code.namespace': ns, 'code.file.path': rel, 'code.line.number': frame.f_lineno}
