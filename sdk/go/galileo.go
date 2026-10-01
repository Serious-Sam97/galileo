// Package galileo is the Go SDK for Galileo: OpenTelemetry traces exported over OTLP/HTTP with
// what plain OpenTelemetry lacks — identity (user.id, tenant.id) on every span, the calling
// function and file:line on SQL and cache spans, panics and errors shaped so Issues can group
// them, and headers for the LLM gateway.
//
//	tel, err := galileo.Init(ctx, galileo.ConfigFromEnv())
//	defer tel.Shutdown(context.Background())
//	handler := tel.Middleware()(mux)
//
// Everything is a no-op when no endpoint is configured, so a developer's machine and the test
// suite pay nothing and callers never check for nil.
//
// Go has no runtime patching, so each library is wired explicitly: the HTTP middleware here,
// [github.com/Serious-Sam97/galileo/sdk/go/pgxtrace] for pgx, [github.com/Serious-Sam97/galileo/sdk/go/redistrace]
// for go-redis and [github.com/Serious-Sam97/galileo/sdk/go/chiroute] for chi route names.
package galileo

import (
	"context"
	"errors"
	"fmt"
	"os"
	"strconv"
	"strings"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracehttp"
	"go.opentelemetry.io/otel/propagation"
	"go.opentelemetry.io/otel/sdk/resource"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	"go.opentelemetry.io/otel/trace"
)

// ScopeName is the instrumentation scope of every span this SDK starts.
const ScopeName = "github.com/Serious-Sam97/galileo/sdk/go"

// Version of the SDK, sent as telemetry.sdk.version.
const Version = "0.1.0"

// Config says where to send telemetry and how the service is named. Build it with
// [ConfigFromEnv] and override fields as needed.
type Config struct {
	// Endpoint is the OTLP/HTTP base, e.g. https://galileo-api.example.com/otlp or
	// http://localhost:4318. Empty disables export.
	Endpoint string
	// APIKey is a project key with the ingest scope (glk_…).
	APIKey string
	// Service is service.name; Env is deployment.environment.name; Release is service.version.
	Service string
	Env     string
	Release string
	// SampleRatio keeps this fraction of new traces (0 < r ≤ 1; default 1). Traces that arrive
	// with a sampled traceparent are always continued.
	SampleRatio float64
	// CallSites puts code.function.name / code.file.path / code.line.number of the application
	// frame on SQL and cache spans (default true).
	CallSites bool
	// ResourceAttributes are added to the resource of every span.
	ResourceAttributes []attribute.KeyValue
	// SpanProcessor replaces the OTLP exporter; tests pass a tracetest.SpanRecorder here. When
	// set, Endpoint may be empty.
	SpanProcessor sdktrace.SpanProcessor
}

// ConfigFromEnv reads the variables every Galileo SDK understands:
//
//	GALILEO_ENDPOINT (or GALILEO_OTLP_ENDPOINT, OTEL_EXPORTER_OTLP_ENDPOINT)
//	GALILEO_API_KEY
//	OTEL_SERVICE_NAME (or GALILEO_SERVICE_NAME)
//	GALILEO_ENV, GALILEO_RELEASE (or APP_VERSION)
//	GALILEO_SAMPLE_RATIO, GALILEO_CALL_SITES=0 to disable call sites
func ConfigFromEnv() Config {
	ratio, err := strconv.ParseFloat(os.Getenv("GALILEO_SAMPLE_RATIO"), 64)
	if err != nil {
		ratio = 1
	}
	return Config{
		Endpoint:    firstEnv("GALILEO_ENDPOINT", "GALILEO_OTLP_ENDPOINT", "OTEL_EXPORTER_OTLP_ENDPOINT"),
		APIKey:      os.Getenv("GALILEO_API_KEY"),
		Service:     firstEnv("OTEL_SERVICE_NAME", "GALILEO_SERVICE_NAME"),
		Env:         os.Getenv("GALILEO_ENV"),
		Release:     firstEnv("GALILEO_RELEASE", "APP_VERSION"),
		SampleRatio: ratio,
		CallSites:   os.Getenv("GALILEO_CALL_SITES") != "0",
	}
}

func firstEnv(keys ...string) string {
	for _, k := range keys {
		if v := strings.TrimSpace(os.Getenv(k)); v != "" {
			return v
		}
	}
	return ""
}

// Telemetry owns the tracer provider built by [Init].
type Telemetry struct {
	provider *sdktrace.TracerProvider
	tracer   trace.Tracer
	cfg      Config
}

// callSites is read by the SQL and cache hooks, which do not hold a *Telemetry.
var callSites = true

// Init builds the tracer provider and installs it globally, together with the W3C trace-context
// propagator (so a request from galileo-rum or another service continues its trace). With no
// endpoint and no SpanProcessor the returned Telemetry hands out valid non-recording spans.
func Init(ctx context.Context, cfg Config) (*Telemetry, error) {
	if cfg.Service == "" {
		cfg.Service = "go-app"
	}
	if cfg.SampleRatio <= 0 || cfg.SampleRatio > 1 {
		cfg.SampleRatio = 1
	}
	callSites = cfg.CallSites
	t := &Telemetry{cfg: cfg}
	otel.SetTextMapPropagator(propagation.NewCompositeTextMapPropagator(propagation.TraceContext{}, propagation.Baggage{}))

	processor := cfg.SpanProcessor
	if processor == nil && cfg.Endpoint != "" {
		exporter, err := newExporter(ctx, cfg)
		if err != nil {
			return nil, err
		}
		processor = sdktrace.NewBatchSpanProcessor(exporter)
	}
	if processor != nil {
		t.provider = sdktrace.NewTracerProvider(
			sdktrace.WithResource(newResource(cfg)),
			sdktrace.WithSampler(sdktrace.ParentBased(sdktrace.TraceIDRatioBased(cfg.SampleRatio))),
			// identity first, so the exporter sees spans with user.id/tenant.id already set
			sdktrace.WithSpanProcessor(identityProcessor{}),
			sdktrace.WithSpanProcessor(processor),
		)
		otel.SetTracerProvider(t.provider)
	}
	t.tracer = otel.Tracer(ScopeName, trace.WithInstrumentationVersion(Version))
	return t, nil
}

func newExporter(ctx context.Context, cfg Config) (sdktrace.SpanExporter, error) {
	opts := []otlptracehttp.Option{otlptracehttp.WithEndpointURL(strings.TrimRight(cfg.Endpoint, "/") + "/v1/traces")}
	if cfg.APIKey != "" {
		opts = append(opts, otlptracehttp.WithHeaders(map[string]string{"Authorization": "Bearer " + cfg.APIKey}))
	}
	exporter, err := otlptracehttp.New(ctx, opts...)
	if err != nil {
		return nil, fmt.Errorf("galileo: otlp exporter: %w", err)
	}
	return exporter, nil
}

func newResource(cfg Config) *resource.Resource {
	attrs := []attribute.KeyValue{
		attribute.String("service.name", cfg.Service),
		attribute.String("telemetry.sdk.name", "galileo-go"),
		attribute.String("telemetry.sdk.language", "go"),
		attribute.String("telemetry.sdk.version", Version),
	}
	if cfg.Release != "" {
		attrs = append(attrs, attribute.String("service.version", cfg.Release))
	}
	if cfg.Env != "" {
		attrs = append(attrs, attribute.String("deployment.environment.name", cfg.Env))
	}
	if host, err := os.Hostname(); err == nil {
		attrs = append(attrs, attribute.String("host.name", host))
	}
	return resource.NewSchemaless(append(attrs, cfg.ResourceAttributes...)...)
}

// Tracer is the SDK's tracer; use it for your own spans. Safe on a nil or no-op Telemetry.
func (t *Telemetry) Tracer() trace.Tracer {
	if t == nil || t.tracer == nil {
		return otel.Tracer(ScopeName)
	}
	return t.tracer
}

// Enabled reports whether spans are being recorded and exported.
func (t *Telemetry) Enabled() bool { return t != nil && t.provider != nil }

// Shutdown flushes buffered spans. Call it on exit, with a deadline. Safe on a no-op Telemetry.
func (t *Telemetry) Shutdown(ctx context.Context) error {
	if t == nil || t.provider == nil {
		return nil
	}
	return errors.Join(t.provider.ForceFlush(ctx), t.provider.Shutdown(ctx))
}

// Tracer returns the global tracer under the SDK's scope, for code that has no *Telemetry.
func Tracer() trace.Tracer { return otel.Tracer(ScopeName) }
