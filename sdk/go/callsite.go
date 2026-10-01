package galileo

import (
	"runtime"
	"strings"
	"sync"

	"go.opentelemetry.io/otel/attribute"
)

var (
	skipMu       sync.RWMutex
	skipPrefixes = []string{
		"github.com/Serious-Sam97/galileo/sdk/go",
		"github.com/jackc/",
		"github.com/redis/go-redis",
		"github.com/go-chi/",
		"go.opentelemetry.io/",
	}
)

// SkipCallSitePackages adds package path prefixes that are never reported as the call site —
// your own DB wrapper or repository base, for instance, so the span points at the code that
// called it. The standard library, pgx, go-redis, chi, OpenTelemetry and this SDK are skipped
// already.
func SkipCallSitePackages(prefixes ...string) {
	skipMu.Lock()
	defer skipMu.Unlock()
	skipPrefixes = append(skipPrefixes, prefixes...)
}

// CallSite returns the code.* attributes of the first application frame on the calling
// goroutine's stack: code.function.name, code.namespace (the package path), code.file.path and
// code.line.number. Empty when call sites are disabled or no application frame is found.
func CallSite() []attribute.KeyValue {
	if !callSites {
		return nil
	}
	return appFrame(false)
}

// culprit is the application frame an exception is blamed on — Galileo fingerprints issues by
// type, culprit and route, and cannot parse Go stack traces, so it reads these code.* attributes.
// Inside a recover it is the function that panicked, not the recovery middleware that caught it.
// Recorded even when call sites are disabled: grouping depends on it.
func culprit() []attribute.KeyValue { return appFrame(true) }

func appFrame(afterPanic bool) []attribute.KeyValue {
	pcs := make([]uintptr, 64)
	n := runtime.Callers(3, pcs)
	frames := runtime.CallersFrames(pcs[:n])
	var found []attribute.KeyValue
	for {
		f, more := frames.Next()
		if afterPanic && f.Function == "runtime.gopanic" {
			// everything above belongs to the recovery path: blame the first app frame below
			found = nil
		} else if found == nil && f.Function != "" && !skipped(f.Function) {
			pkg, fn := splitFunction(f.Function)
			found = []attribute.KeyValue{
				attribute.String("code.function.name", fn),
				attribute.String("code.namespace", pkg),
				attribute.String("code.file.path", f.File),
				attribute.Int("code.line.number", f.Line),
			}
			if !afterPanic {
				return found
			}
		}
		if !more {
			return found
		}
	}
}

func skipped(function string) bool {
	pkg, _ := splitFunction(function)
	// Standard library: no dot in the first path element ("net/http", "database/sql", "runtime").
	if first, _, _ := strings.Cut(pkg, "/"); !strings.Contains(first, ".") && pkg != "main" {
		return true
	}
	skipMu.RLock()
	defer skipMu.RUnlock()
	for _, p := range skipPrefixes {
		// path-aware: "github.com/go-chi" skips github.com/go-chi/chi/v5 but not github.com/go-chix
		p = strings.TrimSuffix(p, "/")
		if pkg == p || strings.HasPrefix(pkg, p+"/") {
			return true
		}
	}
	return false
}

// splitFunction turns "github.com/acme/shop/orders.(*Service).List.func1" into
// ("github.com/acme/shop/orders", "(*Service).List.func1").
func splitFunction(full string) (pkg, fn string) {
	slash := strings.LastIndexByte(full, '/')
	dot := strings.IndexByte(full[slash+1:], '.')
	if dot < 0 {
		return full, full
	}
	return full[:slash+1+dot], full[slash+2+dot:]
}
