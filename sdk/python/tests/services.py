"""Application code: what the call-site attribution should point at."""
from tests.models import Pet


def load_pets(species):
    return list(Pet.objects.filter(species=species))


class Billing:
    def close_invoice(self, invoice_id, secret=None):
        return f'closed {invoice_id}'

    def explode(self):
        raise ValueError('boom')
