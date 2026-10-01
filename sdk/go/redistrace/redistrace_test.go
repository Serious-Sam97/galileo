package redistrace_test

import (
	"context"
	"errors"
	"testing"

	"github.com/redis/go-redis/v9"
	"github.com/stretchr/testify/require"
	sdkmetric "go.opentelemetry.io/otel/sdk/metric"
	"go.opentelemetry.io/otel/sdk/metric/metricdata"
	"go.opentelemetry.io/otel/sdk/trace/tracetest"

	galileo "github.com/Serious-Sam97/galileo/sdk/go"
	"github.com/Serious-Sam97/galileo/sdk/go/redistrace"
)

func TestCommandsAndPipelinesBecomeSpansAndAMissIsNotAnError(t *testing.T) {
	rec := tracetest.NewSpanRecorder()
	tel, err := galileo.Init(context.Background(), galileo.Config{SpanProcessor: rec})
	require.NoError(t, err)
	defer func() { _ = tel.Shutdown(context.Background()) }()
	ctx, parent := tel.Tracer().Start(context.Background(), "GET /x")
	defer parent.End()

	h := redistrace.Hook{}
	miss := h.ProcessHook(func(context.Context, redis.Cmder) error { return redis.Nil })
	require.ErrorIs(t, miss(ctx, redis.NewStringCmd(ctx, "get", "k")), redis.Nil)

	boom := errors.New("connection reset")
	pipe := h.ProcessPipelineHook(func(context.Context, []redis.Cmder) error { return boom })
	require.ErrorIs(t, pipe(ctx, []redis.Cmder{redis.NewStringCmd(ctx, "get", "a"), redis.NewIntCmd(ctx, "incr", "b")}), boom)

	spans := rec.Ended()
	require.Len(t, spans, 2)
	require.Equal(t, "redis get", spans[0].Name())
	require.Empty(t, spans[0].Events(), "a cache miss is an answer")
	require.Equal(t, "redis get incr", spans[1].Name())
	require.Len(t, spans[1].Events(), 1)
	require.Equal(t, parent.SpanContext().SpanID(), spans[0].Parent().SpanID())
}

func TestCommandsAreTimedWithoutASpanAndPipelinesShareOneSeries(t *testing.T) {
	reader := sdkmetric.NewManualReader()
	tel, err := galileo.Init(context.Background(), galileo.Config{Metrics: true, MetricReader: reader})
	require.NoError(t, err)
	defer func() { _ = tel.Shutdown(context.Background()) }()
	ctx := context.Background()

	h := redistrace.Hook{}
	_ = h.ProcessHook(func(context.Context, redis.Cmder) error { return redis.Nil })(ctx, redis.NewStringCmd(ctx, "get", "k"))
	_ = h.ProcessPipelineHook(func(context.Context, []redis.Cmder) error { return nil })(ctx, []redis.Cmder{redis.NewStringCmd(ctx, "get", "a"), redis.NewIntCmd(ctx, "incr", "b")})
	rdb := redis.NewClient(&redis.Options{Addr: "127.0.0.1:1"})
	defer rdb.Close()
	require.NoError(t, redistrace.RecordPoolStats(rdb, "cache"))

	var rm metricdata.ResourceMetrics
	require.NoError(t, reader.Collect(ctx, &rm))
	var ops []string
	var pool bool
	for _, sm := range rm.ScopeMetrics {
		for _, m := range sm.Metrics {
			switch m.Name {
			case galileo.MetricDBDuration:
				for _, p := range m.Data.(metricdata.Histogram[float64]).DataPoints {
					op, _ := p.Attributes.Value("db.operation.name")
					_, failed := p.Attributes.Value("error.type")
					require.False(t, failed, "a miss is not a failure")
					ops = append(ops, op.AsString())
				}
			case "db.client.connection.count":
				pool = true
			}
		}
	}
	require.ElementsMatch(t, []string{"get", "pipeline"}, ops)
	require.True(t, pool)
}
