"""
Finding "the application frame": the first stack frame that is not the framework, the SDK or
a library. That frame is what people mean by "which function ran this query".
"""
import os
import sys
import sysconfig
from types import FrameType
from typing import Optional

_SKIP_MODULE_PREFIXES = (
    'galileo_django', 'opentelemetry', 'django', 'rest_framework', 'psycopg2', 'psycopg',
    'requests', 'urllib3', 'logging', 'asyncio', 'threading', 'concurrent', 'wsgiref', 'gunicorn', 'uvicorn',
)
_LIB_DIRS = tuple(p for p in {sysconfig.get_paths().get('purelib'), sysconfig.get_paths().get('platlib'), sysconfig.get_paths().get('stdlib')} if p)


def is_app_frame(frame: FrameType) -> bool:
    module = frame.f_globals.get('__name__', '') or ''
    if module.split('.')[0] in _SKIP_MODULE_PREFIXES or module.startswith(_SKIP_MODULE_PREFIXES):
        return False
    filename = frame.f_code.co_filename
    if not filename or filename.startswith('<'):
        return False
    if any(filename.startswith(d) for d in _LIB_DIRS) or 'site-packages' in filename or 'dist-packages' in filename:
        return False
    return True


def app_frame(start: Optional[FrameType] = None, max_depth: int = 60) -> Optional[FrameType]:
    frame = start or sys._getframe(1)
    depth = 0
    while frame is not None and depth < max_depth:
        if is_app_frame(frame):
            return frame
        frame = frame.f_back
        depth += 1
    return None


def code_attributes(frame: FrameType, base_dir: Optional[str] = None) -> dict:
    """OpenTelemetry `code.*` attributes for a frame."""
    code = frame.f_code
    module = frame.f_globals.get('__name__', '') or ''
    qualname = getattr(code, 'co_qualname', code.co_name)
    filepath = code.co_filename
    if base_dir and filepath.startswith(base_dir):
        filepath = os.path.relpath(filepath, base_dir)
    namespace = module
    if '.' in qualname:  # method: qualname is Class.method
        namespace = f'{module}.{qualname.rsplit(".", 1)[0]}'
    return {
        'code.function.name': qualname.rsplit('.', 1)[-1],
        'code.namespace': namespace,
        'code.file.path': filepath,
        'code.line.number': frame.f_lineno,
    }


def callable_attributes(fn, base_dir: Optional[str] = None) -> dict:
    """`code.*` attributes for a function object (decorators, views)."""
    code = getattr(fn, '__code__', None)
    module = getattr(fn, '__module__', '') or ''
    qualname = getattr(fn, '__qualname__', getattr(fn, '__name__', 'function'))
    filepath = code.co_filename if code else ''
    if base_dir and filepath.startswith(base_dir):
        filepath = os.path.relpath(filepath, base_dir)
    namespace = f'{module}.{qualname.rsplit(".", 1)[0]}' if '.' in qualname else module
    out = {'code.function.name': qualname.rsplit('.', 1)[-1], 'code.namespace': namespace}
    if filepath:
        out['code.file.path'] = filepath
    if code:
        out['code.line.number'] = code.co_firstlineno
    return out
