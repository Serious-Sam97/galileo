// Package pgxtrace traces pgx v5 statements: one client span per query under the request span,
// with the statement (cut at 1 KB, parameters never recorded) and the application function that
// ran it.
//
//	cfg, _ := pgxpool.ParseConfig(url)
//	cfg.ConnConfig.Tracer = pgxtrace.New()
//	pool, _ := pgxpool.NewWithConfig(ctx, cfg)
package pgxtrace

import (
	"context"
	"errors"

	"github.com/jackc/pgx/v5"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/trace"

	galileo "github.com/Serious-Sam97/galileo/sdk/go"
)

// Tracer implements pgx.QueryTracer.
type Tracer struct {
	// Observer, when set, sees every statement with its context before it runs, whether or not
	// a span is recording (e.g. to count statements per request in a diagnostic).
	Observer func(ctx context.Context, sql string)
}

var _ pgx.QueryTracer = (*Tracer)(nil)

// New returns a tracer for pgx.ConnConfig.Tracer.
func New() *Tracer { return &Tracer{} }

type startedKey struct{}

// TraceQueryStart opens the statement's span.
func (t *Tracer) TraceQueryStart(ctx context.Context, conn *pgx.Conn, data pgx.TraceQueryStartData) context.Context {
	if t.Observer != nil {
		t.Observer(ctx, data.SQL)
	}
	if !trace.SpanFromContext(ctx).IsRecording() {
		return ctx
	}
	var attrs []attribute.KeyValue
	if conn != nil {
		attrs = append(attrs, attribute.String("db.namespace", conn.Config().Database))
	}
	ctx, _ = galileo.StartQuery(ctx, "postgresql", data.SQL, attrs...)
	return context.WithValue(ctx, startedKey{}, true)
}

// TraceQueryEnd closes it; pgx.ErrNoRows is an answer, not a failure.
func (t *Tracer) TraceQueryEnd(ctx context.Context, _ *pgx.Conn, data pgx.TraceQueryEndData) {
	if ctx.Value(startedKey{}) == nil {
		return // no span of ours to close: never end the caller's
	}
	galileo.EndQuery(trace.SpanFromContext(ctx), data.Err, func(err error) bool { return errors.Is(err, pgx.ErrNoRows) })
}
