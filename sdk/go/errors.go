package galileo

import (
	"context"
	"errors"
	"fmt"
	"reflect"
	"runtime/debug"

	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/codes"
	"go.opentelemetry.io/otel/trace"
)

// RecordPanic records a recovered panic on the span in ctx so Issues groups it. Call it from
// your own recovery middleware or goroutine guards:
//
//	defer func() {
//		if p := recover(); p != nil {
//			galileo.RecordPanic(ctx, p)
//			http.Error(w, "internal error", 500)
//		}
//	}()
//
// The exception is typed "panic" rather than after the Go value: otherwise every panic in the
// process ("*errors.errorString") would collapse into a single issue. Galileo groups by type,
// message and route from there.
func RecordPanic(ctx context.Context, p any) {
	span := trace.SpanFromContext(ctx)
	span.AddEvent("exception", trace.WithAttributes(
		attribute.String("exception.type", "panic"),
		attribute.String("exception.message", fmt.Sprint(p)),
		attribute.String("exception.stacktrace", string(debug.Stack())),
	))
	span.SetStatus(codes.Error, "panic")
	span.SetAttributes(culprit()...)
}

// RecordError records err on the span in ctx as an exception Issues can group, and marks the
// span failed. The exception type is the innermost wrapped error's type (e.g. *pgconn.PgError),
// which groups better than the *fmt.wrapError on top. No-op for a nil error.
func RecordError(ctx context.Context, err error, attrs ...attribute.KeyValue) {
	if err == nil {
		return
	}
	span := trace.SpanFromContext(ctx)
	span.AddEvent("exception", trace.WithAttributes(append([]attribute.KeyValue{
		attribute.String("exception.type", errorType(err)),
		attribute.String("exception.message", err.Error()),
		attribute.String("exception.stacktrace", string(debug.Stack())),
	}, attrs...)...))
	span.SetStatus(codes.Error, err.Error())
	span.SetAttributes(culprit()...)
}

func errorType(err error) string {
	for {
		next := errors.Unwrap(err)
		if next == nil {
			return reflect.TypeOf(err).String()
		}
		err = next
	}
}

// Job runs fn inside a span for work that has no HTTP request — queue consumers, cron jobs,
// per-tenant loops — so its SQL and cache spans hang together instead of arriving as loose
// single-span traces. An error or a panic marks the span failed and is recorded; the panic is
// re-raised.
//
//	err := galileo.Job(ctx, "send-reminders", func(ctx context.Context) error { … },
//		attribute.String("tenant.id", clinic))
func Job(ctx context.Context, name string, fn func(context.Context) error, attrs ...attribute.KeyValue) (err error) {
	ctx, span := Tracer().Start(ctx, "job "+name,
		trace.WithSpanKind(trace.SpanKindConsumer),
		trace.WithAttributes(append([]attribute.KeyValue{attribute.String("job.name", name)}, attrs...)...))
	defer func() {
		p := recover()
		if p != nil {
			RecordPanic(ctx, p)
		}
		// End before re-panicking, or End records the panic a second time (see Middleware).
		span.End()
		if p != nil {
			panic(p)
		}
	}()
	err = fn(ctx)
	RecordError(ctx, err)
	return err
}
