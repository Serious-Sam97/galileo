from django.apps import AppConfig


class GalileoConfig(AppConfig):
    name = 'galileo_django'
    label = 'galileo'
    verbose_name = 'Galileo'

    def ready(self):
        from .config import setup
        setup()
