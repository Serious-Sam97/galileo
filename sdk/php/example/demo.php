<?php
// Plain-PHP demo: GALILEO_ENDPOINT / GALILEO_API_KEY / OTEL_SERVICE_NAME, then `php example/demo.php`.
require __DIR__ . '/../vendor/autoload.php';

use Galileo\Galileo;
use Galileo\Identity;
use OpenTelemetry\API\Trace\SpanKind;

Galileo::init();
Identity::set(7, 'vet@example.com', 'Dra. Vet', 'aumiau');

function loadPatients(): int
{
    $t = microtime(true);
    usleep(15000);
    Galileo::query('select * from `patients` where clinic_id = ?', (microtime(true) - $t) * 1000, 'mysql', 'mysql', [3]);
    return 12;
}

Galileo::span('GET /patients', function () {
    $n = loadPatients();
    Galileo::span('render list', fn () => usleep(2000), SpanKind::KIND_INTERNAL, ['patients.count' => $n]);
    try {
        throw new RuntimeException('cart is null');
    } catch (Throwable $e) {
        Galileo::exception($e);
    }
}, SpanKind::KIND_SERVER, ['http.route' => '/patients', 'http.request.method' => 'GET']);
echo "sent\n";
