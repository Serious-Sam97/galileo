// Package pgxtrace traces pgx v5 statements: one client span per query under the request span,
// with the statement (cut at 1 KB, parameters never recorded) and the application function that
// ran it. Every statement, sampled or not, is also timed in db.client.operation.duration, and
// [RecordPoolStats] reports the pool's connections.
//
//	cfg, _ := pgxpool.ParseConfig(url)
//	cfg.ConnConfig.Tracer = pgxtrace.New()
//	pool, _ := pgxpool.NewWithConfig(ctx, cfg)
//	_ = pgxtrace.RecordPoolStats(pool)
package pgxtrace

import (
	"context"
	"errors"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
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
	if !trace.SpanFromContext(ctx).IsRecording() && !galileo.MetricsEnabled() {
		return ctx
	}
	var attrs []attribute.KeyValue
	if conn != nil {
		attrs = append(attrs, attribute.String("db.namespace", conn.Config().Database))
	}
	ctx, span := galileo.StartQuery(ctx, "postgresql", data.SQL, attrs...)
	return context.WithValue(ctx, startedKey{}, span)
}

// TraceQueryEnd closes it; pgx.ErrNoRows is an answer, not a failure.
func (t *Tracer) TraceQueryEnd(ctx context.Context, _ *pgx.Conn, data pgx.TraceQueryEndData) {
	span, ok := ctx.Value(startedKey{}).(trace.Span)
	if !ok {
		return // no span of ours to close: never end the caller's
	}
	galileo.EndQuery(span, data.Err, func(err error) bool { return errors.Is(err, pgx.ErrNoRows) })
}

// RecordPoolStats reports pool's connections at every metric export: db.client.connection.count
// (state idle or used), db.client.connection.max, and the acquisitions that had to wait for a free
// connection (db.client.connection.waits) or gave up (db.client.connection.timeouts). Used close
// to max with waits climbing means the pool, not the database, is the bottleneck.
//
// Call it after galileo.Init; without metric export it does nothing.
func RecordPoolStats(pool *pgxpool.Pool) error {
	m := galileo.Meter()
	name := attribute.String("db.client.connection.pool.name", pool.Config().ConnConfig.Database)
	count, err1 := m.Int64ObservableUpDownCounter("db.client.connection.count", metric.WithUnit("{connection}"), metric.WithDescription("Connections by state"))
	limit, err2 := m.Int64ObservableUpDownCounter("db.client.connection.max", metric.WithUnit("{connection}"), metric.WithDescription("Pool size limit"))
	waits, err3 := m.Int64ObservableCounter("db.client.connection.waits", metric.WithUnit("{acquire}"), metric.WithDescription("Acquisitions that waited for a free connection"))
	timeouts, err4 := m.Int64ObservableCounter("db.client.connection.timeouts", metric.WithUnit("{acquire}"), metric.WithDescription("Acquisitions canceled before a connection was free"))
	if err := errors.Join(err1, err2, err3, err4); err != nil {
		return err
	}
	idle := metric.WithAttributes(name, attribute.String("db.client.connection.state", "idle"))
	used := metric.WithAttributes(name, attribute.String("db.client.connection.state", "used"))
	only := metric.WithAttributes(name)
	_, err := m.RegisterCallback(func(_ context.Context, o metric.Observer) error {
		s := pool.Stat()
		o.ObserveInt64(count, int64(s.IdleConns()), idle)
		o.ObserveInt64(count, int64(s.AcquiredConns()), used)
		o.ObserveInt64(limit, int64(s.MaxConns()), only)
		o.ObserveInt64(waits, s.EmptyAcquireCount(), only)
		o.ObserveInt64(timeouts, s.CanceledAcquireCount(), only)
		return nil
	}, count, limit, waits, timeouts)
	return err
}
