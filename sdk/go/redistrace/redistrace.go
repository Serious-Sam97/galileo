// Package redistrace traces go-redis v9 commands: one client span per command (or pipeline)
// under the request span, with the application function that issued it. Arguments are never
// recorded — keys and values often hold addresses, tokens or tenant names.
//
//	rdb := redis.NewClient(opts)
//	rdb.AddHook(redistrace.Hook{})
package redistrace

import (
	"context"
	"errors"
	"net"
	"strings"

	"github.com/redis/go-redis/v9"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/codes"
	"go.opentelemetry.io/otel/trace"

	galileo "github.com/Serious-Sam97/galileo/sdk/go"
)

// Hook implements redis.Hook.
type Hook struct{}

var _ redis.Hook = Hook{}

// DialHook passes dials through untraced.
func (Hook) DialHook(next redis.DialHook) redis.DialHook {
	return func(ctx context.Context, network, addr string) (net.Conn, error) {
		return next(ctx, network, addr)
	}
}

// ProcessHook wraps a single command.
func (h Hook) ProcessHook(next redis.ProcessHook) redis.ProcessHook {
	return func(ctx context.Context, cmd redis.Cmder) error {
		if !trace.SpanFromContext(ctx).IsRecording() {
			return next(ctx, cmd)
		}
		ctx, span := start(ctx, cmd.Name())
		defer span.End()
		err := next(ctx, cmd)
		record(span, err)
		return err
	}
}

// ProcessPipelineHook wraps a pipeline as one span named after its commands.
func (h Hook) ProcessPipelineHook(next redis.ProcessPipelineHook) redis.ProcessPipelineHook {
	return func(ctx context.Context, cmds []redis.Cmder) error {
		if !trace.SpanFromContext(ctx).IsRecording() {
			return next(ctx, cmds)
		}
		names := make([]string, 0, len(cmds))
		for _, cmd := range cmds {
			names = append(names, cmd.Name())
		}
		ctx, span := start(ctx, strings.Join(names, " "))
		defer span.End()
		err := next(ctx, cmds)
		record(span, err)
		return err
	}
}

func start(ctx context.Context, operation string) (context.Context, trace.Span) {
	attrs := append([]attribute.KeyValue{
		attribute.String("db.system", "redis"),
		attribute.String("db.system.name", "redis"),
		attribute.String("db.operation.name", operation),
	}, galileo.CallSite()...)
	return galileo.Tracer().Start(ctx, "redis "+operation, trace.WithSpanKind(trace.SpanKindClient), trace.WithAttributes(attrs...))
}

func record(span trace.Span, err error) {
	// redis.Nil is a cache miss: an answer, not a failure.
	if err == nil || errors.Is(err, redis.Nil) {
		return
	}
	span.RecordError(err)
	span.SetStatus(codes.Error, err.Error())
}
