"""
Galileo for Django. See README. Public surface:

    galileo_django.setup()            # called for you by the app config; safe to call twice
    galileo_django.traced             # decorator: one span per call, with code.* attributes
    galileo_django.trace_modules(...) # wrap public functions of modules with `traced`
"""
from .config import setup, is_enabled
from .tracing import traced, trace_modules

__all__ = ['setup', 'is_enabled', 'traced', 'trace_modules']
__version__ = '0.2.2'

default_app_config = 'galileo_django.apps.GalileoConfig'
from . import agent  # noqa: E402,F401
