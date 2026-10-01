package redistrace_test

import (
	"context"
	"errors"
	"testing"

	"github.com/redis/go-redis/v9"
	"github.com/stretchr/testify/require"
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
