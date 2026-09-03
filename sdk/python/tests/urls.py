from django.urls import path
from rest_framework.routers import DefaultRouter

from tests.views import PetViewSet, plain

router = DefaultRouter()
router.register('pets', PetViewSet, basename='pet')
urlpatterns = [path('plain/', plain, name='plain'), path('api/', __import__('django.urls').urls.include(router.urls))]
