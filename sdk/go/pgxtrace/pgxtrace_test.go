package pgxtrace_test

import (
	"context"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/stretchr/testify/require"
	sdkmetric "go.opentelemetry.io/otel/sdk/metric"
	"go.opentelemetry.io/otel/sdk/metric/metricdata"
	"go.opentelemetry.io/otel/sdk/trace/tracetest"

	galileo "github.com/Serious-Sam97/galileo/sdk/go"
	"github.com/Serious-Sam97/galileo/sdk/go/pgxtrace"
)

func TestOneChildPerStatementAndTheParentIsNeverClosed(t *testing.T) {
	rec := tracetest.NewSpanRecorder()
	tel, err := galileo.Init(context.Background(), galileo.Config{SpanProcessor: rec, CallSites: true})
	require.NoError(t, err)
	defer func() { _ = tel.Shutdown(context.Background()) }()

	var seen []string
	tr := &pgxtrace.Tracer{Observer: func(_ context.Context, sql string) { seen = append(seen, sql) }}
	ctx, parent := tel.Tracer().Start(context.Background(), "GET /x")

	qctx := tr.TraceQueryStart(ctx, nil, pgx.TraceQueryStartData{SQL: "select * from consultas where id = $1"})
	tr.TraceQueryEnd(qctx, nil, pgx.TraceQueryEndData{Err: pgx.ErrNoRows})
	require.Len(t, rec.Ended(), 1)
	child := rec.Ended()[0]
	require.Equal(t, "SELECT consultas", child.Name())
	require.Equal(t, parent.SpanContext().SpanID(), child.Parent().SpanID())
	require.Empty(t, child.Events(), "no rows is not an error")
	require.Equal(t, []string{"select * from consultas where id = $1"}, seen)

	// An End with no matching Start must not end the caller's span.
	tr.TraceQueryEnd(ctx, nil, pgx.TraceQueryEndData{})
	require.True(t, parent.IsRecording())
	parent.End()
	require.Len(t, rec.Ended(), 2)
}

func TestAFailedStatementIsAnError(t *testing.T) {
	rec := tracetest.NewSpanRecorder()
	tel, _ := galileo.Init(context.Background(), galileo.Config{SpanProcessor: rec})
	defer func() { _ = tel.Shutdown(context.Background()) }()
	tr := pgxtrace.New()
	ctx, parent := tel.Tracer().Start(context.Background(), "GET /x")
	defer parent.End()
	qctx := tr.TraceQueryStart(ctx, nil, pgx.TraceQueryStartData{SQL: "insert into pets values ($1)"})
	tr.TraceQueryEnd(qctx, nil, pgx.TraceQueryEndData{Err: context.DeadlineExceeded})
	child := rec.Ended()[0]
	require.Equal(t, "INSERT pets", child.Name())
	require.Len(t, child.Events(), 1)
}

func TestStatementsAreTimedWithoutASpanAndThePoolIsReported(t *testing.T) {
	reader := sdkmetric.NewManualReader()
	tel, err := galileo.Init(context.Background(), galileo.Config{Metrics: true, MetricReader: reader})
	require.NoError(t, err)
	defer func() { _ = tel.Shutdown(context.Background()) }()

	tr := pgxtrace.New()
	qctx := tr.TraceQueryStart(context.Background(), nil, pgx.TraceQueryStartData{SQL: "select * from consultas"})
	tr.TraceQueryEnd(qctx, nil, pgx.TraceQueryEndData{})

	// no connection is opened until the pool is used
	pool, err := pgxpool.New(context.Background(), "postgres://u:p@127.0.0.1:1/melea?pool_max_conns=4")
	require.NoError(t, err)
	defer pool.Close()
	require.NoError(t, pgxtrace.RecordPoolStats(pool))

	var rm metricdata.ResourceMetrics
	require.NoError(t, reader.Collect(context.Background(), &rm))
	got := map[string]metricdata.Metrics{}
	for _, sm := range rm.ScopeMetrics {
		for _, m := range sm.Metrics {
			got[m.Name] = m
		}
	}
	require.Len(t, got[galileo.MetricDBDuration].Data.(metricdata.Histogram[float64]).DataPoints, 1)
	limit := got["db.client.connection.max"].Data.(metricdata.Sum[int64]).DataPoints
	require.Equal(t, int64(4), limit[0].Value)
	require.Len(t, got["db.client.connection.count"].Data.(metricdata.Sum[int64]).DataPoints, 2)
}
