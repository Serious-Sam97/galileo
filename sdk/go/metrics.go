package galileo

import (
	"context"
	"fmt"
	"strings"
	"sync/atomic"
	"time"

	"go.opentelemetry.io/contrib/instrumentation/host"
	"go.opentelemetry.io/contrib/instrumentation/runtime"
	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/exporters/otlp/otlpmetric/otlpmetrichttp"
	"go.opentelemetry.io/otel/metric"
	"go.opentelemetry.io/otel/metric/noop"
	sdkmetric "go.opentelemetry.io/otel/sdk/metric"
	"go.opentelemetry.io/otel/sdk/metric/metricdata"
)

// Metrics recorded by the SDK. Durations are in milliseconds, like galileo-django's, so a board
// mixing Go and Python services compares like with like.
//
// They are recorded for every request, statement and job, sampled or not: traces may keep one in
// five, but request counts, error rates and latencies stay exact.
const (
	MetricServerDuration = "http.server.request.duration" // histogram, ms
	MetricServerRequests = "http.server.requests"         // counter
	MetricServerActive   = "http.server.active_requests"  // up-down counter
	MetricClientDuration = "http.client.request.duration" // histogram, ms
	MetricDBDuration     = "db.client.operation.duration" // histogram, ms (SQL and Redis)
	MetricJobDuration    = "job.duration"                 // histogram, ms
)

// msBuckets suit request, statement and job latencies from sub-millisecond cache hits to slow
// reports.
var msBuckets = []float64{1, 2.5, 5, 10, 25, 50, 75, 100, 250, 500, 750, 1000, 2500, 5000, 10000, 30000}

type instruments struct {
	meter          metric.Meter
	serverDuration metric.Float64Histogram
	serverRequests metric.Int64Counter
	serverActive   metric.Int64UpDownCounter
	clientDuration metric.Float64Histogram
	dbDuration     metric.Float64Histogram
	jobDuration    metric.Float64Histogram
}

// active holds the instruments of the current [Init]; nil means metrics are off and recording
// costs a pointer load. The pgxtrace and redistrace hooks reach it without a *Telemetry.
var active atomic.Pointer[instruments]

func newInstruments(m metric.Meter) (*instruments, error) {
	var (
		in  = &instruments{meter: m}
		err error
		e   error
	)
	hist := func(name, desc string) metric.Float64Histogram {
		h, e2 := m.Float64Histogram(name, metric.WithUnit("ms"), metric.WithDescription(desc), metric.WithExplicitBucketBoundaries(msBuckets...))
		if e2 != nil {
			err = e2
		}
		return h
	}
	in.serverDuration = hist(MetricServerDuration, "Request latency")
	in.clientDuration = hist(MetricClientDuration, "Outgoing HTTP request latency")
	in.dbDuration = hist(MetricDBDuration, "Database and cache operation latency")
	in.jobDuration = hist(MetricJobDuration, "Background job duration")
	if in.serverRequests, e = m.Int64Counter(MetricServerRequests, metric.WithUnit("{request}"), metric.WithDescription("Requests handled")); e != nil {
		err = e
	}
	if in.serverActive, e = m.Int64UpDownCounter(MetricServerActive, metric.WithUnit("{request}"), metric.WithDescription("Requests in flight")); e != nil {
		err = e
	}
	return in, err
}

// Meter is the SDK's meter, for your own instruments (orders placed, queue depth). It is a
// no-op until [Init] has set up metric export.
func Meter() metric.Meter {
	if in := active.Load(); in != nil {
		return in.meter
	}
	return noop.Meter{}
}

// deltaTemporality sends counters and histograms as per-interval deltas: Galileo charts each
// point on its own (SUM of requests, mean latency per bucket), which cumulative totals since
// process start would turn into a running average. Up-down counters and gauges stay absolute.
func deltaTemporality(k sdkmetric.InstrumentKind) metricdata.Temporality {
	switch k {
	case sdkmetric.InstrumentKindCounter, sdkmetric.InstrumentKindHistogram, sdkmetric.InstrumentKindObservableCounter:
		return metricdata.DeltaTemporality
	default:
		return metricdata.CumulativeTemporality
	}
}

func newMeterProvider(ctx context.Context, cfg Config) (*sdkmetric.MeterProvider, error) {
	active.Store(nil) // the latest Init decides, as it does for call sites
	reader := cfg.MetricReader
	if reader == nil {
		if cfg.Endpoint == "" || !cfg.Metrics {
			return nil, nil
		}
		opts := []otlpmetrichttp.Option{
			otlpmetrichttp.WithEndpointURL(strings.TrimRight(cfg.Endpoint, "/") + "/v1/metrics"),
			otlpmetrichttp.WithTemporalitySelector(deltaTemporality),
		}
		if cfg.APIKey != "" {
			opts = append(opts, otlpmetrichttp.WithHeaders(map[string]string{"Authorization": "Bearer " + cfg.APIKey}))
		}
		exporter, err := otlpmetrichttp.New(ctx, opts...)
		if err != nil {
			return nil, fmt.Errorf("galileo: otlp metric exporter: %w", err)
		}
		interval := cfg.MetricsInterval
		if interval <= 0 {
			interval = 60 * time.Second
		}
		reader = sdkmetric.NewPeriodicReader(exporter, sdkmetric.WithInterval(interval))
	}
	provider := sdkmetric.NewMeterProvider(sdkmetric.WithResource(newResource(cfg)), sdkmetric.WithReader(reader))
	otel.SetMeterProvider(provider)

	in, err := newInstruments(provider.Meter(ScopeName, metric.WithInstrumentationVersion(Version)))
	if err != nil {
		return nil, fmt.Errorf("galileo: instruments: %w", err)
	}
	// go.goroutine.count, go.memory.used, go.memory.gc.goal, go.schedule.duration, …
	if err := runtime.Start(runtime.WithMeterProvider(provider)); err != nil {
		return nil, fmt.Errorf("galileo: runtime metrics: %w", err)
	}
	// process.cpu.time, system.cpu.*, system.memory.*, system.network.io
	if err := host.Start(host.WithMeterProvider(provider)); err != nil {
		return nil, fmt.Errorf("galileo: host metrics: %w", err)
	}
	active.Store(in)
	return provider, nil
}

// RecordDBOperation records one database or cache operation's duration under
// db.client.operation.duration. pgxtrace, redistrace and [EndQuery] call it; call it yourself for
// a driver you time by hand. attrs should be low-cardinality: db.system.name,
// db.operation.name, db.collection.name. A non-nil err adds error.type.
func RecordDBOperation(ctx context.Context, d time.Duration, err error, attrs ...attribute.KeyValue) {
	in := active.Load()
	if in == nil {
		return
	}
	if err != nil {
		attrs = append(attrs, attribute.String("error.type", errorType(err)))
	}
	in.dbDuration.Record(ctx, ms(d), metric.WithAttributes(attrs...))
}

// MetricsEnabled reports whether metrics are being exported, so instrumentation can skip the
// work of timing an unsampled call when nobody records it.
func MetricsEnabled() bool { return active.Load() != nil }

func ms(d time.Duration) float64 { return float64(d) / float64(time.Millisecond) }
