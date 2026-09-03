"""`@traced` and `trace_modules`: application functions as spans."""
import functools
import importlib
import inspect
import os
import pkgutil
import sys

from opentelemetry import trace
from opentelemetry.trace import SpanKind, Status, StatusCode

from .frames import callable_attributes
from .redact import redact_value

_tracer = trace.get_tracer('galileo_django.app')


def traced(fn=None, *, name=None, capture_args=(), kind=SpanKind.INTERNAL):
    """
    One span per call. Records the argument *names* passed (`code.args`), the values only for
    names in `capture_args` (redacted, truncated), the return type, and exceptions.

        @traced
        def fechar_fatura(fatura_id, user): ...

        @traced(capture_args=['fatura_id'])
        def fechar_fatura(fatura_id, user): ...
    """
    def decorate(func):
        attrs = callable_attributes(func, _base_dir())
        span_name = name or f"{attrs['code.namespace']}.{attrs['code.function.name']}" if attrs.get('code.namespace') else attrs['code.function.name']
        sig = None
        try:
            sig = inspect.signature(func)
        except (TypeError, ValueError):
            pass

        def _attributes(args, kwargs):
            a = dict(attrs)
            if sig is not None:
                try:
                    bound = sig.bind_partial(*args, **kwargs)
                    names = [n for n in bound.arguments if n not in ('self', 'cls')]
                    a['code.args'] = ','.join(names)
                    for n in capture_args:
                        if n in bound.arguments:
                            a[f'code.arg.{n}'] = redact_value(bound.arguments[n])
                except TypeError:
                    pass
            return a

        if inspect.iscoroutinefunction(func):
            @functools.wraps(func)
            async def async_wrapper(*args, **kwargs):
                with _tracer.start_as_current_span(span_name, kind=kind, attributes=_attributes(args, kwargs)) as span:
                    try:
                        result = await func(*args, **kwargs)
                    except Exception as exc:
                        span.record_exception(exc)
                        span.set_status(Status(StatusCode.ERROR, f'{type(exc).__name__}: {exc}'[:200]))
                        raise
                    span.set_attribute('code.return_type', type(result).__name__)
                    return result
            async_wrapper.__galileo_traced__ = True
            return async_wrapper

        @functools.wraps(func)
        def wrapper(*args, **kwargs):
            with _tracer.start_as_current_span(span_name, kind=kind, attributes=_attributes(args, kwargs)) as span:
                try:
                    result = func(*args, **kwargs)
                except Exception as exc:
                    span.record_exception(exc)
                    span.set_status(Status(StatusCode.ERROR, f'{type(exc).__name__}: {exc}'[:200]))
                    raise
                span.set_attribute('code.return_type', type(result).__name__)
                return result
        wrapper.__galileo_traced__ = True
        return wrapper

    return decorate(fn) if fn is not None else decorate


def _base_dir():
    try:
        from django.conf import settings
        return str(getattr(settings, 'BASE_DIR', '') or '')
    except Exception:  # noqa: BLE001
        return ''


def trace_modules(patterns):
    """
    Wrap every public function *defined in* the listed modules with `traced`. Patterns:
    `myapp.services` (one module) or `billing.*` (a package and all its submodules).
    Classes are wrapped method by method (public, defined in the module).
    """
    wrapped = 0
    for pattern in patterns:
        pattern = pattern.strip()
        if not pattern:
            continue
        names = []
        if pattern.endswith('.*'):
            root = pattern[:-2]
            try:
                pkg = importlib.import_module(root)
            except ImportError:
                continue
            names.append(root)
            if hasattr(pkg, '__path__'):
                names.extend(m.name for m in pkgutil.walk_packages(pkg.__path__, prefix=root + '.'))
        else:
            names.append(pattern)
        for modname in names:
            try:
                module = importlib.import_module(modname)
            except ImportError:
                continue
            wrapped += _wrap_module(module)
    return wrapped


def _wrap_module(module):
    count = 0
    for attr_name, obj in list(vars(module).items()):
        if attr_name.startswith('_'):
            continue
        if inspect.isfunction(obj) and obj.__module__ == module.__name__ and not getattr(obj, '__galileo_traced__', False):
            setattr(module, attr_name, traced(obj))
            count += 1
        elif inspect.isclass(obj) and obj.__module__ == module.__name__:
            for meth_name, meth in list(vars(obj).items()):
                if meth_name.startswith('_') or not inspect.isfunction(meth) or getattr(meth, '__galileo_traced__', False):
                    continue
                setattr(obj, meth_name, traced(meth))
                count += 1
    return count
