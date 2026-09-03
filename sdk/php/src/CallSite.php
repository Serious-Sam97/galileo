<?php
declare(strict_types=1);

namespace Galileo;

/**
 * Which application function issued this query / call: the first stack frame outside vendor/,
 * the framework and this package, as OTel code.* attributes.
 */
final class CallSite
{
    /** @var string[] path fragments that never count as application code */
    public static array $skip = ['/vendor/', 'Illuminate/', 'Laravel/', 'symfony/'];

    /** @return array<string, string|int> */
    public static function capture(?string $root = null, int $limit = 40): array
    {
        $root ??= (string) getcwd();
        $frames = debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS, $limit);
        foreach ($frames as $i => $f) {
            $file = $f['file'] ?? '';
            if ($file === '') continue;
            $isApp = !str_starts_with($file, __DIR__); // this package's own src/ never counts
            foreach (self::$skip as $s) { if (str_contains($file, $s)) { $isApp = false; break; } }
            if (!$isApp) continue;
            // the function *of this frame* is in the next frame's 'function'/'class'
            $next = $frames[$i + 1] ?? [];
            $fn = $next['function'] ?? 'main';
            $class = $next['class'] ?? '';
            $rel = str_starts_with($file, $root) ? ltrim(substr($file, strlen($root)), '/\\') : $file;
            return [
                'code.function.name' => $fn,
                'code.namespace' => $class !== '' ? $class : preg_replace('/\.php$/', '', str_replace(['/', '\\'], '.', $rel)),
                'code.file.path' => $rel,
                'code.line.number' => (int) ($f['line'] ?? 0),
            ];
        }
        return [];
    }
}
