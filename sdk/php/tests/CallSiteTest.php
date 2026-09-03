<?php
declare(strict_types=1);

namespace Galileo\Tests;

use Galileo\CallSite;
use Galileo\Galileo;
use Galileo\Identity;
use PHPUnit\Framework\TestCase;

final class CallSiteTest extends TestCase
{
    private function loadOrders(): array { return CallSite::capture(dirname(__DIR__)); }

    public function testCaptureFindsTheAppFrame(): void
    {
        $cs = $this->loadOrders();
        self::assertSame('loadOrders', $cs['code.function.name']);
        self::assertSame(self::class, $cs['code.namespace']);
        self::assertStringEndsWith('CallSiteTest.php', $cs['code.file.path']);
        self::assertGreaterThan(0, $cs['code.line.number']);
    }

    public function testIdentityAttributes(): void
    {
        Identity::set(42, 'a@b.c', 'Ann', 'acme');
        self::assertSame(['user.id' => '42', 'user.email' => 'a@b.c', 'user.name' => 'Ann', 'tenant.id' => 'acme'], Identity::attributes());
        Identity::clear();
        self::assertSame([], Identity::attributes());
    }

    public function testTableOf(): void
    {
        self::assertSame('orders', Galileo::tableOf('select * from `orders` where id = ?'));
        self::assertSame('public.users', Galileo::tableOf('UPDATE public.users SET x = 1'));
        self::assertSame('', Galileo::tableOf('SHOW TABLES'));
    }
}
