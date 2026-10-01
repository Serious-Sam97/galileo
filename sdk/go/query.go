package galileo

import (
	"context"
	"regexp"
	"strings"
	"time"
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
// The statement is also timed in db.client.operation.duration (system, operation, table) when
// metrics are on, even when ctx has no recording span; with neither, it does no work and returns
// a span whose End is a no-op.
func StartQuery(ctx context.Context, system, sql string, attrs ...attribute.KeyValue) (context.Context, trace.Span) {
	recording, metrics := trace.SpanFromContext(ctx).IsRecording(), MetricsEnabled()
	if !recording && !metrics {
		return ctx, trace.SpanFromContext(ctx)
	}
	sql = strings.TrimSpace(sql)
	op, table := Operation(sql), Table(sql)
	shape := []attribute.KeyValue{
		attribute.String("db.system.name", system),
		attribute.String("db.operation.name", op),
	}
	if table != "" {
		shape = append(shape, attribute.String("db.collection.name", table))
	}
	span := trace.SpanFromContext(context.Background()) // no-op: never end the caller's span
	if recording {
		name := op
		if table != "" {
			name = op + " " + table
		}
		all := append([]attribute.KeyValue{
			attribute.String("db.system", system),
			attribute.String("db.query.text", truncate(sql, MaxStatement)),
		}, shape...)
		all = append(all, CallSite()...)
		ctx, span = Tracer().Start(ctx, name, trace.WithSpanKind(trace.SpanKindClient), trace.WithAttributes(append(all, attrs...)...))
	}
	if !metrics {
		return ctx, span
	}
	return ctx, &timedSpan{Span: span, start: time.Now(), attrs: shape}
}

// timedSpan carries what EndQuery needs to record the statement's duration.
type timedSpan struct {
	trace.Span
	start time.Time
	attrs []attribute.KeyValue
}

// EndQuery ends a span from [StartQuery], recording err unless ignore(err) says it is an
// answer rather than a failure (e.g. "no rows").
func EndQuery(span trace.Span, err error, ignore ...func(error) bool) {
	if err != nil {
		for _, f := range ignore {
			if f(err) {
				err = nil
				break
			}
		}
	}
	if ts, ok := span.(*timedSpan); ok {
		RecordDBOperation(context.Background(), time.Since(ts.start), err, ts.attrs...)
		span = ts.Span
	}
	if err != nil && span.IsRecording() {
		RecordError(trace.ContextWithSpan(context.Background(), span), err)
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
