"""The current request, for code that runs outside the middleware call chain (logging)."""
import threading

_state = threading.local()


def set_current_request(request):
    _state.request = request


def current_request():
    return getattr(_state, 'request', None)
