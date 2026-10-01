package galileo

import (
	"context"
	"regexp"
	"strings"
	"unicode/utf8"

	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/trace"
)

// MaxStatement is where db.query.text is cut. Parameters are never recorded.
const MaxStatement = 1024

var tableRe = regexp.MustCompile(`(?i)\b(?:from|into|update|join)\s+[` + "`" + `"\[]?([A-Za-z0-9_.]+)[` + "`" + `"\]]?`)

// Operation is the statement's leading keyword, upper-cased ("SELECT", "INSERT", "WITH").
func Operation(sql string) string {
	f := strings.Fields(sql)
	if len(f) == 0 {
		return "QUERY"
	}
	return strings.ToUpper(f[0])
}

// Table is the first table the statement reads or writes, or "".
func Table(sql string) string {
	if m := tableRe.FindStringSubmatch(sql); m != nil {
		return m[1]
	}
	return ""
}

// StartQuery opens a client span for one database statement under the span in ctx, named
// "SELECT orders", with db.* attributes (current and legacy spellings) and the application call
// site. The pgxtrace package uses it; call it yourself around database/sql or any other driver:
//
//	ctx, span := galileo.StartQuery(ctx, "postgresql", q)
//	rows, err := db.QueryContext(ctx, q, args...)
//	galileo.EndQuery(span, err)
//
// Returns a non-recording span (and does no work) when ctx has no recording span.
func StartQuery(ctx context.Context, system, sql string, attrs ...attribute.KeyValue) (context.Context, trace.Span) {
	if !trace.SpanFromContext(ctx).IsRecording() {
		return ctx, trace.SpanFromContext(ctx)
	}
	sql = strings.TrimSpace(sql)
	op, table := Operation(sql), Table(sql)
	name := op
	all := []attribute.KeyValue{
		attribute.String("db.system", system),
		attribute.String("db.system.name", system),
		attribute.String("db.query.text", truncate(sql, MaxStatement)),
		attribute.String("db.operation.name", op),
	}
	if table != "" {
		name = op + " " + table
		all = append(all, attribute.String("db.collection.name", table))
	}
	all = append(all, CallSite()...)
	return Tracer().Start(ctx, name, trace.WithSpanKind(trace.SpanKindClient), trace.WithAttributes(append(all, attrs...)...))
}

// EndQuery ends a span from [StartQuery], recording err unless ignore(err) says it is an
// answer rather than a failure (e.g. "no rows").
func EndQuery(span trace.Span, err error, ignore ...func(error) bool) {
	if err != nil {
		failed := true
		for _, f := range ignore {
			if f(err) {
				failed = false
			}
		}
		if failed {
			RecordError(trace.ContextWithSpan(context.Background(), span), err)
		}
	}
	span.End()
}

func truncate(s string, n int) string {
	if len(s) <= n {
		return s
	}
	cut := n
	for cut > 0 && !utf8.RuneStart(s[cut]) {
		cut--
	}
	return s[:cut] + "…"
}
