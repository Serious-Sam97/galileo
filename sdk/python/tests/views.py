from django.http import JsonResponse
from rest_framework.response import Response
from rest_framework.viewsets import ViewSet

from tests.services import load_pets


class PetViewSet(ViewSet):
    def list(self, request):
        pets = load_pets(request.GET.get('species', 'dog'))
        return Response([{'id': p.id, 'name': p.name} for p in pets])

    def retrieve(self, request, pk=None):
        if pk == '13':
            raise RuntimeError('unlucky pet')
        return Response({'id': int(pk)})


def plain(request):
    return JsonResponse({'ok': True})
