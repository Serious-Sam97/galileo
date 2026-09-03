import logging

from opentelemetry.sdk._logs.export import InMemoryLogExporter, SimpleLogRecordProcessor

import galileo


def test_logs_carry_identity_and_trace():
    galileo.init(endpoint='http://localhost:1', api_key='x', service='t')
    ex = InMemoryLogExporter()
    galileo._state['logger_provider'].add_log_record_processor(SimpleLogRecordProcessor(ex))
    galileo.set_identity(user_id='u5', tenant='t5')
    with galileo.tracer().start_as_current_span('req'):
        logging.getLogger('example').info('listing pets')
    recs = [r for r in ex.get_finished_logs() if 'listing pets' in str(r.log_record.body)]
    assert recs, [str(r.log_record.body) for r in ex.get_finished_logs()]
    a = dict(recs[-1].log_record.attributes or {})
    assert a.get('user.id') == 'u5' and a.get('tenant.id') == 't5'
    assert recs[-1].log_record.trace_id
