SECRET_KEY = 'test'
DEBUG = True
ALLOWED_HOSTS = ['*']
INSTALLED_APPS = ['django.contrib.contenttypes', 'django.contrib.auth', 'rest_framework', 'galileo_django', 'tests']
MIDDLEWARE = [
    'galileo_django.middleware.GalileoContextMiddleware',
    'django.contrib.auth.middleware.AuthenticationMiddleware' if False else 'django.middleware.common.CommonMiddleware',
    'galileo_django.middleware.GalileoViewMiddleware',
]
ROOT_URLCONF = 'tests.urls'
DATABASES = {'default': {'ENGINE': 'django.db.backends.sqlite3', 'NAME': ':memory:'}}
BASE_DIR = __file__.rsplit('/', 2)[0]
USE_TZ = True
DEFAULT_AUTO_FIELD = 'django.db.models.BigAutoField'
