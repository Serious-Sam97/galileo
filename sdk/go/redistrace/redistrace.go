// Package redistrace traces go-redis v9 commands: one client span per command (or pipeline)
// under the request span, with the application function that issued it. Arguments are never
// recorded — keys and values often hold addresses, tokens or tenant names. Every command, sampled
// or not, is also timed in db.client.operation.duration, and [RecordPoolStats] reports the pool.
//
//	rdb := redis.NewClient(opts)
//	rdb.AddHook(redistrace.Hook{})
//	_ = redistrace.RecordPoolStats(rdb, "cache")
package redistrace

import (
	"context"
	"errors"
	"net"
	"strings"
	"time"

	"github.com/redis/go-redis/v9"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/codes"
	"go.opentelemetry.io/otel/metric"
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
		recording := trace.SpanFromContext(ctx).IsRecording()
		if !recording && !galileo.MetricsEnabled() {
			return next(ctx, cmd)
		}
		began := time.Now()
		var span trace.Span
		if recording {
			ctx, span = start(ctx, cmd.Name())
			defer span.End()
		}
		err := next(ctx, cmd)
		if recording {
			record(span, err)
		}
		timed(ctx, began, cmd.Name(), err)
		return err
	}
}

// ProcessPipelineHook wraps a pipeline as one span named after its commands.
func (h Hook) ProcessPipelineHook(next redis.ProcessPipelineHook) redis.ProcessPipelineHook {
	return func(ctx context.Context, cmds []redis.Cmder) error {
		recording := trace.SpanFromContext(ctx).IsRecording()
		if !recording && !galileo.MetricsEnabled() {
			return next(ctx, cmds)
		}
		began := time.Now()
		var span trace.Span
		if recording {
			names := make([]string, 0, len(cmds))
			for _, cmd := range cmds {
				names = append(names, cmd.Name())
			}
			ctx, span = start(ctx, strings.Join(names, " "))
			defer span.End()
		}
		err := next(ctx, cmds)
		if recording {
			record(span, err)
		}
		// one series for all pipelines: their command lists would each be a new series
		timed(ctx, began, "pipeline", err)
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

func timed(ctx context.Context, began time.Time, operation string, err error) {
	if errors.Is(err, redis.Nil) {
		err = nil
	}
	galileo.RecordDBOperation(ctx, time.Since(began), err,
		attribute.String("db.system.name", "redis"), attribute.String("db.operation.name", operation))
}

// RecordPoolStats reports the client's connection pool at every metric export:
// db.client.connection.count (state idle or used) and db.client.connection.timeouts (waits for a
// free connection that timed out), under db.client.connection.pool.name = name.
//
// Call it after galileo.Init; without metric export it does nothing.
func RecordPoolStats(c interface{ PoolStats() *redis.PoolStats }, name string) error {
	m := galileo.Meter()
	count, err1 := m.Int64ObservableUpDownCounter("db.client.connection.count", metric.WithUnit("{connection}"), metric.WithDescription("Connections by state"))
	timeouts, err2 := m.Int64ObservableCounter("db.client.connection.timeouts", metric.WithUnit("{acquire}"), metric.WithDescription("Acquisitions canceled before a connection was free"))
	if err := errors.Join(err1, err2); err != nil {
		return err
	}
	pool := attribute.String("db.client.connection.pool.name", name)
	idle := metric.WithAttributes(pool, attribute.String("db.client.connection.state", "idle"))
	used := metric.WithAttributes(pool, attribute.String("db.client.connection.state", "used"))
	_, err := m.RegisterCallback(func(_ context.Context, o metric.Observer) error {
		s := c.PoolStats()
		o.ObserveInt64(count, int64(s.IdleConns), idle)
		o.ObserveInt64(count, int64(s.TotalConns)-int64(s.IdleConns), used)
		o.ObserveInt64(timeouts, int64(s.Timeouts), metric.WithAttributes(pool))
		return nil
	}, count, timeouts)
	return err
}

func record(span trace.Span, err error) {
	// redis.Nil is a cache miss: an answer, not a failure.
	if err == nil || errors.Is(err, redis.Nil) {
		return
	}
	span.RecordError(err)
	span.SetStatus(codes.Error, err.Error())
}
