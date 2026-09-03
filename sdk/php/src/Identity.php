<?php
declare(strict_types=1);

namespace Galileo;

/** Who is acting (per request); attached to every span and log record. */
final class Identity
{
    /** @var array<string, string> */
    private static array $attrs = [];

    public static function set(string|int|null $id, ?string $email = null, ?string $name = null, string|int|null $tenant = null): void
    {
        self::$attrs = [];
        if ($id !== null) self::$attrs['user.id'] = (string) $id;
        if ($email) self::$attrs['user.email'] = $email;
        if ($name) self::$attrs['user.name'] = $name;
        if ($tenant !== null) self::$attrs['tenant.id'] = (string) $tenant;
    }

    public static function clear(): void { self::$attrs = []; }

    /** @return array<string, string> */
    public static function attributes(): array { return self::$attrs; }
}
